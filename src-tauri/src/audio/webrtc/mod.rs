//! Collaborative-audio node: bridges the DSP graph to remote peers over WebRTC
//! DataChannels carrying Opus. The receive path (jitter buffer, per-rate fan-out)
//! is shared with direct-IP audio in `crate::audio::stream_recv`.

mod channels;
mod handshake;
mod registry;
mod session;
mod tasks;

pub use handshake::{accept_offer, complete_handshake, create_offer};
pub use registry::{
    disconnect_peer, get_or_create, leave_room, mark_room, peer_pings, session_state,
    set_identity, set_peer_muted, set_signaling_task, WebRtcSessionState,
};

// Shared receive tap map (jitter buffer, broadcast, fan-out task live in stream_recv).
pub use crate::audio::stream_recv::TapMap as PeerSnapshotMap;

// Opus operates only at 48 kHz; the DSP graph runs at the output device rate,
// so the encode and receive paths resample to/from 48 kHz.
pub const OPUS_SR: u32 = 48_000;
pub const OPUS_FRAME_SAMPLES: usize = 960 * 2;
pub const RESAMPLE_CHUNK: usize = 256;
pub const STUN_URL: &str = "stun:stun.l.google.com:19302";
pub const AUDIO_CHANNEL: &str = "audio";
