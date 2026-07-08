use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use rtrb::{Consumer, Producer};
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::RTCPeerConnection;

use crate::audio::graph::OpusApplication;

use super::{PeerSnapshotMap, OPUS_SR};

pub struct WebRtcSession {
    #[allow(dead_code)]
    pub node_id: String,
    pub opus_bitrate: u32,
    pub opus_application: OpusApplication,
    pub send_consumer: Mutex<Option<Consumer<f32>>>,
    pub peer_snapshots: PeerSnapshotMap,
    pub peers: tokio::sync::Mutex<HashMap<String, Arc<PeerState>>>,
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
    pub recv_producer: Mutex<Option<Producer<f32>>>,
    pub decoder: Mutex<opus::Decoder>,
    pub recv_snapshot: Arc<Mutex<Vec<f32>>>,
    pub muted: Arc<AtomicBool>,
    #[allow(dead_code)]
    pub ping_ms: Arc<AtomicU32>,
    // The peer ID to show in the UI — the *remote* side's identity.
    // Host: starts as connection_id, updated to guestPeerId after complete_handshake.
    // Guest: set to connection_id (= host's ID) at creation.
    pub display_id: Arc<Mutex<String>>,
}

impl WebRtcSession {
    pub fn new(node_id: String, opus_bitrate: u32, opus_application: OpusApplication) -> Self {
        Self {
            node_id,
            opus_bitrate,
            opus_application,
            send_consumer: Mutex::new(None),
            peer_snapshots: Arc::new(Mutex::new(HashMap::new())),
            peers: tokio::sync::Mutex::new(HashMap::new()),
            output_sr: Arc::new(AtomicU32::new(OPUS_SR)),
            encoder_started: AtomicBool::new(false),
            phase: Mutex::new("idle"),
            room_code: Mutex::new(None),
            signaling_task: Mutex::new(None),
        }
    }

    pub fn set_send_consumer(&self, consumer: Consumer<f32>, output_sr: u32) -> PeerSnapshotMap {
        *self.send_consumer.lock().unwrap() = Some(consumer);
        self.output_sr.store(output_sr, Ordering::Relaxed);
        self.peer_snapshots.clone()
    }
}
