use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use rtrb::{Consumer, Producer};
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::RTCPeerConnection;

use crate::audio::graph::{NetCodec, OpusApplication};
use crate::audio::netaudio::codec::ChannelDecoder;
use crate::audio::netaudio::packet::Format;
use crate::audio::stream_recv::{ChannelBroadcast, ConsumerHandle, FanoutRegistry};

use super::OPUS_SR;

pub struct WebRtcSession {
    #[allow(dead_code)]
    pub node_id: String,
    pub opus_bitrate: u32,
    pub opus_application: OpusApplication,
    // One send ring per local channel; the encode task drains all of them and
    // tags each Opus packet with its channel index.
    pub send_consumers: Mutex<Vec<Consumer<f32>>>,
    // Each output subgraph builds its own bridge, so received audio fans out to
    // per-bridge rings (keyed "peer:ch") rather than being drained once.
    pub fanout: FanoutRegistry,
    pub peers: tokio::sync::Mutex<HashMap<String, Arc<PeerState>>>,
    // Local participant name and input count, shared over the ctrl channel.
    pub local_name: Arc<Mutex<String>>,
    pub local_channels: Arc<AtomicU32>,
    // Wire codec (packet::Format byte). Live-read by the encode loop, so the UI
    // can switch Opus/PCM without reconnecting. Packets are self-describing, so
    // the receiver adapts per packet regardless of this setting.
    pub codec: AtomicU8,
    // DSP graph rate the bridge feeds/reads at; the async paths resample it to
    // 48 kHz for Opus. Defaults to 48 kHz until the bridge is instantiated.
    pub output_sr: Arc<AtomicU32>,
    // Guard so only one encode loop runs regardless of how many peers connect.
    pub encoder_started: AtomicBool,
    // "idle" | "hosting" | "joining".
    pub phase: Mutex<&'static str>,
    pub room_code: Mutex<Option<String>>,
    // In-flight signaling exchange, aborted when the user cancels the room.
    pub signaling_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

pub struct PeerState {
    pub peer_id: String,
    pub pc: Arc<RTCPeerConnection>,
    pub dc: Mutex<Option<Arc<RTCDataChannel>>>,
    // Ctrl channel, kept so name changes can be re-broadcast after connect.
    pub ctrl_dc: Mutex<Option<Arc<RTCDataChannel>>>,
    // Receive state is created lazily per channel index as packets arrive, so a
    // peer sending N channels is handled without agreeing on N up front.
    pub channels: Mutex<HashMap<u8, Arc<PeerChannel>>>,
    pub muted: Arc<AtomicBool>,
    pub ping_ms: Arc<AtomicU32>,
    // Received audio packets and gaps inferred from per-channel seq numbers, for
    // a receive-side loss ratio in the UI.
    pub packets: Arc<AtomicU64>,
    pub lost: Arc<AtomicU64>,
    // Remote participant name and input count from the peer's ctrl meta.
    pub remote_name: Arc<Mutex<String>>,
    pub remote_channels: Arc<AtomicU32>,
    // The peer ID to show in the UI — the *remote* side's identity.
    // Host: starts as connection_id, updated to guestPeerId after complete_handshake.
    // Guest: set to connection_id (= host's ID) at creation.
    pub display_id: Arc<Mutex<String>>,
}

pub struct PeerChannel {
    // Format-agnostic decoder (Opus or raw PCM, chosen per packet).
    pub decoder: Mutex<ChannelDecoder>,
    pub recv_producer: Mutex<Option<Producer<f32>>>,
    // Last seq seen on this channel, to count gaps as loss.
    pub last_seq: Mutex<Option<u16>>,
}

impl WebRtcSession {
    pub fn new(node_id: String, opus_bitrate: u32, opus_application: OpusApplication) -> Self {
        Self {
            node_id,
            opus_bitrate,
            opus_application,
            send_consumers: Mutex::new(Vec::new()),
            fanout: FanoutRegistry::default(),
            peers: tokio::sync::Mutex::new(HashMap::new()),
            local_name: Arc::new(Mutex::new(String::new())),
            local_channels: Arc::new(AtomicU32::new(1)),
            codec: AtomicU8::new(Format::Opus.to_byte()),
            output_sr: Arc::new(AtomicU32::new(OPUS_SR)),
            encoder_started: AtomicBool::new(false),
            phase: Mutex::new("idle"),
            room_code: Mutex::new(None),
            signaling_task: Mutex::new(None),
        }
    }

    pub fn set_codec(&self, codec: NetCodec) {
        let f = match codec {
            NetCodec::PcmF32 => Format::PcmF32,
            NetCodec::PcmI16 => Format::PcmI16,
            NetCodec::Opus => Format::Opus,
        };
        self.codec.store(f.to_byte(), Ordering::Relaxed);
    }

    pub fn set_send_consumers(&self, consumers: Vec<Consumer<f32>>, output_sr: u32) {
        *self.send_consumers.lock().unwrap() = consumers;
        self.output_sr.store(output_sr, Ordering::Relaxed);
    }

    pub fn register_bridge(&self, output_sr: u32, realtime: bool) -> ConsumerHandle {
        self.fanout.register_consumer(output_sr, realtime)
    }

    /// New received channel (keyed `peer:channel`), wired into every live bridge.
    pub fn attach_channel(&self, peer: String, channel: u8) -> ChannelBroadcast {
        self.fanout.attach_channel(format!("{peer}:{channel}"))
    }

    /// Drops a disconnected peer's channels so new bridges don't wire to them.
    pub fn drop_peer_channels(&self, peer: &str) {
        self.fanout.drop_prefix(&format!("{peer}:"));
    }
}
