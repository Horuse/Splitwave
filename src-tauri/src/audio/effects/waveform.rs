use std::sync::{Arc, Mutex};

use crate::audio::graph::WaveformData;

use super::Effect;

pub const WAVEFORM_FRAMES: usize = 1024;

struct WaveformState {
    buf: Box<[f32]>, // interleaved L/R, len = WAVEFORM_FRAMES * 2
    write: usize,    // write head in frames
}

impl WaveformState {
    fn new() -> Self {
        Self {
            buf: vec![0.0_f32; WAVEFORM_FRAMES * 2].into_boxed_slice(),
            write: 0,
        }
    }
}

#[derive(Clone)]
pub struct WaveformHandle {
    pub node_id: String,
    state: Arc<Mutex<WaveformState>>,
}

pub struct WaveformEffect {
    handle: WaveformHandle,
}

impl WaveformHandle {
    fn new(node_id: String) -> Self {
        Self {
            node_id,
            state: Arc::new(Mutex::new(WaveformState::new())),
        }
    }

    /// Returns the last WAVEFORM_FRAMES frames as a chronologically ordered
    /// interleaved L/R buffer. Called from the meter tick thread (non-RT).
    pub fn snapshot(&self) -> Box<[f32]> {
        let g = self.state.lock().unwrap();
        let pos = g.write * 2;
        let mut out = vec![0.0_f32; WAVEFORM_FRAMES * 2].into_boxed_slice();
        let first_len = WAVEFORM_FRAMES * 2 - pos;
        out[..first_len].copy_from_slice(&g.buf[pos..]);
        out[first_len..].copy_from_slice(&g.buf[..pos]);
        out
    }
}

impl WaveformEffect {
    pub fn new(_d: WaveformData, node_id: String) -> (Self, WaveformHandle) {
        let handle = WaveformHandle::new(node_id);
        (Self { handle: handle.clone() }, handle)
    }

    pub fn from_handle(handle: WaveformHandle) -> Self {
        Self { handle }
    }
}

impl Effect for WaveformEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        // try_lock: a miss means this display block is skipped -- acceptable.
        if let Ok(mut g) = self.handle.state.try_lock() {
            let n = frames.min(WAVEFORM_FRAMES);
            let src = &samples[..n * 2];
            let byte_pos = g.write * 2;
            let end = byte_pos + n * 2;
            if end <= WAVEFORM_FRAMES * 2 {
                g.buf[byte_pos..end].copy_from_slice(src);
                g.write = if end == WAVEFORM_FRAMES * 2 { 0 } else { g.write + n };
            } else {
                let first_samps = WAVEFORM_FRAMES * 2 - byte_pos;
                g.buf[byte_pos..].copy_from_slice(&src[..first_samps]);
                let second_samps = n * 2 - first_samps;
                g.buf[..second_samps].copy_from_slice(&src[first_samps..]);
                g.write = second_samps / 2;
            }
        }
    }
}
