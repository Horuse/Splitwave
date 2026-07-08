//! Collaborative-audio node: bridges the DSP graph to remote peers over WebRTC
//! DataChannels carrying Opus.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

mod channels;
mod handshake;
mod registry;
mod session;
mod tasks;

pub use handshake::{accept_offer, complete_handshake, create_offer};
pub use registry::{
    disconnect_peer, get_or_create, leave_room, mark_room, peer_pings, session_state,
    set_peer_muted, set_signaling_task, WebRtcSessionState,
};

// Opus operates only at 48 kHz; the DSP graph runs at the output device rate
// (`output_sr`), so the encode and receive paths resample to/from 48 kHz.
pub const OPUS_SR: u32 = 48_000;
// 20 ms Opus frames at 48 kHz stereo.
pub const OPUS_FRAME_SAMPLES: usize = 960 * 2;
pub const RESAMPLE_CHUNK: usize = 256;
pub const STUN_URL: &str = "stun:stun.l.google.com:19302";
pub const AUDIO_CHANNEL: &str = "audio";
pub const RECV_RING: usize = 48_000;
// Snapshot holds at most one DSP block so the RT bridge can copy it whole.
pub const SNAPSHOT_MAX: usize = 2048;

pub type PeerSnapshotMap = Arc<Mutex<HashMap<String, Arc<Mutex<Vec<f32>>>>>>;
