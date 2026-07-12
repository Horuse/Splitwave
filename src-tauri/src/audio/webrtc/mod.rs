//! Collaborative-audio node: bridges the DSP graph to remote peers over WebRTC
//! DataChannels carrying Opus.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rtrb::Consumer;

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

// Opus operates only at 48 kHz; the DSP graph runs at the output device rate
// (`output_sr`), so the encode and receive paths resample to/from 48 kHz.
pub const OPUS_SR: u32 = 48_000;
pub const OPUS_FRAME_SAMPLES: usize = 960 * 2;
pub const RESAMPLE_CHUNK: usize = 256;
pub const STUN_URL: &str = "stun:stun.l.google.com:19302";
pub const AUDIO_CHANNEL: &str = "audio";
pub const RECV_RING: usize = 48_000;
pub const PLAYBACK_RING: usize = 48_000;
// Max samples the RT bridge pulls per block (DSP_BLOCK_FRAMES * 2 channels).
pub const PLAYBACK_SCRATCH: usize = 2048;
// Buffer this many samples (~40 ms stereo @ 48k) before playback starts, and
// re-buffer after a full drain, so network jitter doesn't stutter the output.
pub const PLAYBACK_PRIME: usize = 4096;
// Above this backlog (~120 ms) sender/receiver drift has piled up, so skip
// ahead to PLAYBACK_PRIME to keep end-to-end latency bounded.
pub const PLAYBACK_MAX: usize = 12_288;

pub type PeerSnapshotMap = Arc<Mutex<HashMap<String, PlaybackTap>>>;

/// RT-side jitter buffer for one peer. The snapshot task pushes resampled,
/// output-rate audio into the ring; the bridge pops one block per callback into
/// `scratch` (read by both the mixed sum and the per-peer handle).
pub struct PlaybackTap {
    consumer: Consumer<f32>,
    pub scratch: Vec<f32>,
    pub valid: usize,
    primed: bool,
    // Remote peer display id and channel index this tap carries, for routing to
    // the bridge's per-channel / per-peer output handles.
    pub peer: String,
    pub channel: u8,
}

impl PlaybackTap {
    pub fn new(consumer: Consumer<f32>, peer: String, channel: u8) -> Self {
        Self {
            consumer,
            scratch: vec![0.0; PLAYBACK_SCRATCH],
            valid: 0,
            primed: false,
            peer,
            channel,
        }
    }

    /// Pop up to `block_len` samples into `scratch`, applying the jitter
    /// buffer. Returns the count written; the rest of `scratch` is stale and
    /// callers must read only `..valid`.
    pub fn fill_block(&mut self, block_len: usize) -> usize {
        let need = block_len.min(self.scratch.len());
        let mut avail = self.consumer.slots();
        if !self.primed {
            if avail < PLAYBACK_PRIME {
                self.valid = 0;
                return 0;
            }
            self.primed = true;
        }
        if avail == 0 {
            self.primed = false;
            self.valid = 0;
            return 0;
        }
        // Drop drift backlog (even sample count so channels stay aligned).
        if avail > PLAYBACK_MAX {
            let drop = (avail - PLAYBACK_PRIME) & !1;
            if let Ok(chunk) = self.consumer.read_chunk(drop) {
                chunk.commit_all();
            }
            avail = self.consumer.slots();
        }
        let n = need.min(avail);
        if let Ok(chunk) = self.consumer.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            self.scratch[..a.len()].copy_from_slice(a);
            self.scratch[a.len()..a.len() + b.len()].copy_from_slice(b);
            chunk.commit_all();
        }
        self.valid = n;
        n
    }
}
