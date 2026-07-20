use std::sync::{Arc, Mutex};

use crate::audio::graph::WaveformData;

use super::Effect;

pub const WAVEFORM_FRAMES: usize = 1024;

/// Upper bound on scoped channels; sizes the fixed ring so display never
/// allocates on the RT path.
pub const MAX_WAVEFORM_CHANNELS: usize = 64;

struct WaveformState {
    buf: Box<[f32]>, // interleaved, len = WAVEFORM_FRAMES * MAX_WAVEFORM_CHANNELS
    channels: usize,
    write: usize, // frame write head
}

impl WaveformState {
    fn new() -> Self {
        Self {
            buf: vec![0.0_f32; WAVEFORM_FRAMES * MAX_WAVEFORM_CHANNELS].into_boxed_slice(),
            channels: 0,
            write: 0,
        }
    }
}

#[derive(Clone)]
pub struct WaveformHandle {
    pub node_id: String,
    state: Arc<Mutex<WaveformState>>,
}

impl WaveformHandle {
    fn new(node_id: String) -> Self {
        Self {
            node_id,
            state: Arc::new(Mutex::new(WaveformState::new())),
        }
    }

    /// Returns the last WAVEFORM_FRAMES frames as a chronologically ordered
    /// interleaved buffer plus its channel count. Called from the meter tick
    /// thread (non-RT).
    pub fn snapshot(&self) -> (Vec<f32>, usize) {
        let g = self.state.lock().unwrap();
        let ch = g.channels.max(1);
        let used = WAVEFORM_FRAMES * ch;
        let pos = g.write * ch;
        let mut out = vec![0.0_f32; used];
        let first_len = used - pos;
        out[..first_len].copy_from_slice(&g.buf[pos..used]);
        out[first_len..].copy_from_slice(&g.buf[..pos]);
        (out, ch)
    }
}

pub struct WaveformEffect {
    handle: WaveformHandle,
}

impl WaveformEffect {
    pub fn new(_d: WaveformData, node_id: String) -> (Self, WaveformHandle) {
        let handle = WaveformHandle::new(node_id);
        (Self { handle: handle.clone() }, handle)
    }

    pub fn from_handle(handle: WaveformHandle) -> Self {
        Self { handle }
    }

    /// Spectrum nodes capture identically to the scope; the FFT runs in the UI.
    pub fn new_for(node_id: String) -> (Self, WaveformHandle) {
        let handle = WaveformHandle::new(node_id);
        (Self { handle: handle.clone() }, handle)
    }
}

impl Effect for WaveformEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        let ch = (samples.len() / frames).clamp(1, MAX_WAVEFORM_CHANNELS);
        // try_lock: a miss means this display block is skipped -- acceptable.
        if let Ok(mut g) = self.handle.state.try_lock() {
            // Channel count changed: reset the ring rather than misalign strides.
            if g.channels != ch {
                g.channels = ch;
                g.write = 0;
            }
            let cap = WAVEFORM_FRAMES;
            let n = frames.min(cap);
            let src = &samples[..n * ch];
            let pos = g.write;
            let end = pos + n;
            if end <= cap {
                g.buf[pos * ch..end * ch].copy_from_slice(src);
                g.write = if end == cap { 0 } else { end };
            } else {
                let first = (cap - pos) * ch;
                g.buf[pos * ch..cap * ch].copy_from_slice(&src[..first]);
                g.buf[..(n * ch - first)].copy_from_slice(&src[first..]);
                g.write = end - cap;
            }
        }
    }
}
