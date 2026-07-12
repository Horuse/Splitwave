use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};

use rtrb::{Consumer, Producer, RingBuffer};
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::RTCPeerConnection;

use crate::audio::graph::OpusApplication;

use super::{PeerSnapshotMap, PlaybackTap, OPUS_SR, PLAYBACK_RING};

// A received peer-channel fans out to every output bridge: the snapshot task
// pushes resampled audio into each producer here, one per live bridge.
pub type ChannelBroadcast = Arc<Mutex<Vec<Producer<f32>>>>;

pub struct WebRtcSession {
    #[allow(dead_code)]
    pub node_id: String,
    pub opus_bitrate: u32,
    pub opus_application: OpusApplication,
    // One send ring per local channel; the encode task drains all of them and
    // tags each Opus packet with its channel index.
    pub send_consumers: Mutex<Vec<Consumer<f32>>>,
    // Each output subgraph builds its own bridge, so received audio must fan out
    // rather than be drained once. `channel_broadcasts` (keyed "peer:ch") is the
    // fan-out point per received channel; `bridge_taps` are the live bridges'
    // per-channel consumers, wired into every broadcast.
    pub channel_broadcasts: Mutex<HashMap<String, ChannelBroadcast>>,
    pub bridge_taps: Mutex<Vec<Weak<Mutex<HashMap<String, PlaybackTap>>>>>,
    pub peers: tokio::sync::Mutex<HashMap<String, Arc<PeerState>>>,
    // Local participant name and input count, shared over the ctrl channel.
    pub local_name: Arc<Mutex<String>>,
    pub local_channels: Arc<AtomicU32>,
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
    // Remote participant name and input count from the peer's ctrl meta.
    pub remote_name: Arc<Mutex<String>>,
    pub remote_channels: Arc<AtomicU32>,
    // The peer ID to show in the UI — the *remote* side's identity.
    // Host: starts as connection_id, updated to guestPeerId after complete_handshake.
    // Guest: set to connection_id (= host's ID) at creation.
    pub display_id: Arc<Mutex<String>>,
}

pub struct PeerChannel {
    pub decoder: Mutex<opus::Decoder>,
    pub recv_producer: Mutex<Option<Producer<f32>>>,
}

impl WebRtcSession {
    pub fn new(node_id: String, opus_bitrate: u32, opus_application: OpusApplication) -> Self {
        Self {
            node_id,
            opus_bitrate,
            opus_application,
            send_consumers: Mutex::new(Vec::new()),
            channel_broadcasts: Mutex::new(HashMap::new()),
            bridge_taps: Mutex::new(Vec::new()),
            peers: tokio::sync::Mutex::new(HashMap::new()),
            local_name: Arc::new(Mutex::new(String::new())),
            local_channels: Arc::new(AtomicU32::new(1)),
            output_sr: Arc::new(AtomicU32::new(OPUS_SR)),
            encoder_started: AtomicBool::new(false),
            phase: Mutex::new("idle"),
            room_code: Mutex::new(None),
            signaling_task: Mutex::new(None),
        }
    }

    pub fn set_send_consumers(&self, consumers: Vec<Consumer<f32>>, output_sr: u32) {
        *self.send_consumers.lock().unwrap() = consumers;
        self.output_sr.store(output_sr, Ordering::Relaxed);
    }

    /// New output bridge: an empty tap map wired into every known channel's
    /// broadcast, tracked (weakly) so later channels attach to it too.
    /// Locks `bridge_taps` before `channel_broadcasts` (see `attach_channel`).
    pub fn register_bridge(&self) -> PeerSnapshotMap {
        let map: PeerSnapshotMap = Arc::new(Mutex::new(HashMap::new()));
        let mut bridges = self.bridge_taps.lock().unwrap();
        bridges.retain(|w| w.strong_count() > 0);
        for (key, bc) in self.channel_broadcasts.lock().unwrap().iter() {
            let Some((peer, ch)) = parse_key(key) else { continue };
            let (prod, cons) = RingBuffer::<f32>::new(PLAYBACK_RING);
            bc.lock().unwrap().push(prod);
            map.lock()
                .unwrap()
                .insert(key.clone(), PlaybackTap::new(cons, peer, ch));
        }
        bridges.push(Arc::downgrade(&map));
        map
    }

    /// New received channel: a fresh broadcast wired into every live bridge.
    pub fn attach_channel(&self, peer: String, channel: u8) -> ChannelBroadcast {
        let key = format!("{peer}:{channel}");
        let bc: ChannelBroadcast = Arc::new(Mutex::new(Vec::new()));
        let mut bridges = self.bridge_taps.lock().unwrap();
        bridges.retain(|w| w.strong_count() > 0);
        for w in bridges.iter() {
            if let Some(map) = w.upgrade() {
                let (prod, cons) = RingBuffer::<f32>::new(PLAYBACK_RING);
                bc.lock().unwrap().push(prod);
                map.lock()
                    .unwrap()
                    .insert(key.clone(), PlaybackTap::new(cons, peer.clone(), channel));
            }
        }
        self.channel_broadcasts.lock().unwrap().insert(key, bc.clone());
        bc
    }

    /// Drops a disconnected peer's broadcasts so new bridges don't wire to them.
    pub fn drop_peer_channels(&self, peer: &str) {
        let prefix = format!("{peer}:");
        self.channel_broadcasts
            .lock()
            .unwrap()
            .retain(|k, _| !k.starts_with(&prefix));
    }
}

fn parse_key(key: &str) -> Option<(String, u8)> {
    let (peer, ch) = key.rsplit_once(':')?;
    Some((peer.to_string(), ch.parse().ok()?))
}
