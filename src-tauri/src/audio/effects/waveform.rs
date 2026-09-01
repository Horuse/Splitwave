use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::audio::graph::WaveformData;

use super::Effect;

/// Distinguishes recorder sessions in scope/progress payloads: an overwrite
/// restart rewinds the absolute frame counter, so frame arithmetic alone
/// cannot tell a fresh session from a stale tail block of the previous one.
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

/// Scope ring holds several blocks so the 33 ms meter tick never outruns the
/// ~21 ms DSP block rate; a longer tick stall overwrites the oldest frames,
/// which the UI renders as a skipped span.
pub const SCOPE_RING_FRAMES: usize = 16384;

/// Spectrum nodes need a longer contiguous window than the scope: a single
/// 1024-frame block is ~47 Hz/bin and cannot separate low tones. 4096 frames
/// (~11.7 Hz/bin) do, and one snapshot stays gap-free (concatenating separate
/// snapshots would inject discontinuities that smear across the spectrum).
pub const SPECTRUM_FRAMES: usize = 4096;

/// Upper bound on scoped channels; sizes the fixed ring so display never
/// allocates on the RT path.
pub const MAX_WAVEFORM_CHANNELS: usize = 64;

struct WaveformState {
    buf: Box<[f32]>, // interleaved, len = frames * MAX_WAVEFORM_CHANNELS
    frames: usize,
    channels: usize,
    write: usize,  // ring write head (== total % frames)
    total: u64,    // absolute frames written
    emit_pos: u64, // absolute frames already emitted via drain
}

impl WaveformState {
    fn new(frames: usize) -> Self {
        Self {
            buf: vec![0.0_f32; frames * MAX_WAVEFORM_CHANNELS].into_boxed_slice(),
            frames,
            channels: 0,
            write: 0,
            total: 0,
            emit_pos: 0,
        }
    }
}

#[derive(Clone)]
pub struct WaveformHandle {
    pub node_id: String,
    /// Rate the captured samples run at (monitor SR), so the UI can map bins to
    /// frequency without assuming 48 kHz.
    pub sample_rate: u32,
    /// Recording session this handle belongs to; emitted with every scope and
    /// progress payload so the UI can drop state owned by a replaced session.
    pub session: u64,
    /// Pre-existing file frames this session appends to (0 for a fresh or
    /// overwrite write); lets the UI keep disk-backed history across an append.
    pub base_frames: u64,
    state: Arc<Mutex<WaveformState>>,
    spectrum: bool,
}

impl WaveformHandle {
    fn with_frames(node_id: String, sample_rate: u32, frames: usize, spectrum: bool) -> Self {
        Self {
            node_id,
            sample_rate,
            // Every handle owns one timeline: a rebuilt graph must adopt as a
            // new session, or the UI's absolute frame counters rewind under it.
            session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            base_frames: 0,
            state: Arc::new(Mutex::new(WaveformState::new(frames))),
            spectrum,
        }
    }

    /// Scope-size handle; used by the Waveform effect and by non-effect
    /// consumers such as the File Recording node.
    pub fn new(node_id: String, sample_rate: u32) -> Self {
        Self::with_frames(node_id, sample_rate, SCOPE_RING_FRAMES, false)
    }

    /// Scope-size handle for one recorder worker invocation.
    pub fn for_recorder(node_id: String, sample_rate: u32, base_frames: u64) -> Self {
        Self {
            base_frames,
            ..Self::with_frames(node_id, sample_rate, SCOPE_RING_FRAMES, false)
        }
    }

    pub fn is_spectrum(&self) -> bool {
        self.spectrum
    }

    /// Returns the last `frames` frames as a chronologically ordered interleaved
    /// buffer plus its channel count. Called from the meter tick thread (non-RT);
    /// used by the spectrum node, which needs a full contiguous window.
    pub fn snapshot(&self) -> (Vec<f32>, usize) {
        let g = self.state.lock().unwrap();
        let ch = g.channels.max(1);
        let used = g.frames * ch;
        let pos = g.write * ch;
        let mut out = vec![0.0_f32; used];
        let first_len = used - pos;
        out[..first_len].copy_from_slice(&g.buf[pos..used]);
        out[first_len..].copy_from_slice(&g.buf[..pos]);
        (out, ch)
    }

    /// Returns the frames written since the previous call, in chronological
    /// order, plus the absolute frame index of the first sample. Scopes consume
    /// this delta (rather than the whole ring) so consecutive ticks neither
    /// overlap nor skip. Called from the meter tick thread (non-RT).
    pub fn drain(&self) -> (u64, Vec<f32>, usize) {
        let mut g = self.state.lock().unwrap();
        let ch = g.channels.max(1);
        let avail = g.total.saturating_sub(g.emit_pos) as usize;
        let cap = g.frames;
        let n = avail.min(cap);
        let start = g.total - n as u64;
        let mut out = vec![0.0_f32; n * ch];
        for i in 0..n {
            let slot = ((start + i as u64) % cap as u64) as usize;
            out[i * ch..(i + 1) * ch].copy_from_slice(&g.buf[slot * ch..(slot + 1) * ch]);
        }
        g.emit_pos = g.total;
        (start, out, ch)
    }

    /// Ingests an interleaved block from a non-RT thread (the recorder worker).
    /// Blocks on the state lock, unlike the effect's `try_lock` path.
    /// `base_frames` seeds the absolute frame counter on the first block, so
    /// `drain` reports file-absolute start positions even when the UI scopes
    /// a recording that appended onto existing content.
    pub fn push_interleaved(&self, samples: &[f32], frames: usize, base_frames: u64) {
        if frames == 0 {
            return;
        }
        let mut g = self.state.lock().unwrap();
        // write() resets the counters when it latches the channel stride, so the
        // append base must be applied after it.
        let seed = g.total == 0 && g.emit_pos == 0 && base_frames > 0;
        write(&mut g, samples, frames);
        if seed {
            g.total = base_frames + frames as u64;
            g.emit_pos = base_frames;
        }
    }
}

/// Writes one interleaved block into a `WaveformState`; `channels` is derived
/// from the stride and a change resets the ring (and its absolute counter)
/// rather than misaligning it.
fn write(g: &mut WaveformState, samples: &[f32], frames: usize) {
    let ch = (samples.len() / frames).clamp(1, MAX_WAVEFORM_CHANNELS);
    if g.channels != ch {
        g.channels = ch;
        g.write = 0;
        g.total = 0;
        g.emit_pos = 0;
    }
    let cap = g.frames;
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
    g.total += n as u64;
}

pub struct WaveformEffect {
    handle: WaveformHandle,
}

impl WaveformEffect {
    pub fn new(_d: WaveformData, node_id: String, sample_rate: u32) -> (Self, WaveformHandle) {
        let handle = WaveformHandle::new(node_id, sample_rate);
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }

    pub fn from_handle(handle: WaveformHandle) -> Self {
        Self { handle }
    }

    /// Spectrum nodes capture identically to the scope, just over a longer
    /// contiguous window; the FFT runs in the UI.
    pub fn new_for(node_id: String, sample_rate: u32) -> (Self, WaveformHandle) {
        let handle = WaveformHandle::with_frames(node_id, sample_rate, SPECTRUM_FRAMES, true);
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }
}

impl Effect for WaveformEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        // try_lock: a miss means this display block is skipped -- acceptable.
        if let Ok(mut g) = self.handle.state.try_lock() {
            write(&mut g, samples, frames);
        }
    }
}
