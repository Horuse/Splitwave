//! Collaborative-audio node: bridges the DSP graph to remote peers over WebRTC
//! DataChannels carrying Opus. The receive path (jitter buffer, per-rate fan-out)
//! is shared with direct-IP audio in `crate::audio::stream_recv`.

mod channels;
mod handshake;
mod registry;
mod session;
mod tasks;

pub use handshake::{
    accept_offer, accept_offer_trickle, add_remote_candidate, apply_answer, complete_handshake,
    create_offer, create_offer_trickle,
};
pub use registry::{
    buffer_ms, disconnect_peer, get_or_create, leave_room, mark_room, peer_pings, peer_stats,
    session_state, set_identity, set_peer_muted, set_signaling_task, WebRtcSessionState,
};

// Opus operates only at 48 kHz; the DSP graph runs at the output device rate,
// so the encode and receive paths resample to/from 48 kHz.
pub const OPUS_SR: u32 = 48_000;
pub const RESAMPLE_CHUNK: usize = 256;
pub const STUN_URL: &str = "stun:stun.l.google.com:19302";
pub const AUDIO_CHANNEL: &str = "audio";
