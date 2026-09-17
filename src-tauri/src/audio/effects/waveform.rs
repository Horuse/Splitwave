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
        // Align the ring head with the file-absolute counter: drain maps
        // absolute frames to `(start + i) % cap` slots, so without this the
        // seeded block would be written at 0..n but read back as silence.
        let head = seed.then(|| (base_frames as usize) % g.frames);
        write(&mut g, samples, frames, head);
        if seed {
            g.total = base_frames + frames as u64;
            g.emit_pos = base_frames;
        }
    }
}

/// Writes one interleaved block into a `WaveformState`; `channels` is derived
/// from the stride and a change resets the ring (and its absolute counter)
/// rather than misaligning it. `head_seed` overrides the write head after the
/// reset (used by append-mode seeding so the block lands where `drain`
/// expects it).
fn write(g: &mut WaveformState, samples: &[f32], frames: usize, head_seed: Option<usize>) {
    let ch = (samples.len() / frames).clamp(1, MAX_WAVEFORM_CHANNELS);
    if g.channels != ch {
        g.channels = ch;
        g.write = 0;
        g.total = 0;
        g.emit_pos = 0;
    }
    if let Some(head) = head_seed {
        g.write = head;
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
            write(&mut g, samples, frames, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn handle(node: &str, sr: u32) -> WaveformHandle {
        WaveformHandle::new(node.to_string(), sr)
    }

    fn block(k: usize, frames: usize, ch: usize) -> Vec<f32> {
        (0..frames * ch)
            .map(|i| (k as f32) * 1000.0 + i as f32)
            .collect()
    }

    #[test]
    fn snapshot_keeps_ring_layout() {
        let h = handle("n", 48_000);
        let b = block(1, 4, 2);
        h.push_interleaved(&b, 4, 0);
        // Snapshot always returns the full ring worth, chronologically
        // arranged from the write head; freshly written frames land last.
        let (out, ch) = h.snapshot();
        assert_eq!(ch, 2);
        assert_eq!(out.len(), SCOPE_RING_FRAMES * 2);
        assert_eq!(&out[out.len() - 8..], b.as_slice());
    }

    #[test]
    fn snapshot_holds_everything_until_ring_fills() {
        let h = handle("n", 48_000);
        for k in 0..3 {
            h.push_interleaved(&block(k, 10, 2), 10, 0);
        }
        let (out, ch) = h.snapshot();
        assert_eq!(ch, 2);
        assert_eq!(out.len(), SCOPE_RING_FRAMES * 2);
        let mut want = block(0, 10, 2);
        want.extend(block(1, 10, 2));
        want.extend(block(2, 10, 2));
        assert_eq!(&out[out.len() - 60..], want.as_slice());
    }

    #[test]
    fn ring_wraps_and_keeps_only_recent_frames() {
        let h = handle("n", 48_000);
        let per = 1000usize;
        let n_blocks = 20usize; // 20000 frames > 16384 ring
        for k in 0..n_blocks {
            h.push_interleaved(&block(k, per, 2), per, 0);
        }
        let (out, ch) = h.snapshot();
        assert_eq!(ch, 2);
        assert_eq!(out.len(), SCOPE_RING_FRAMES * 2);
        // The newest frames must match the last block's values.
        let newest = block(19, per, 2);
        for i in 0..per * 2 {
            assert_eq!(out[out.len() - per * 2 + i], newest[i], "sample {i}");
        }
    }

    #[test]
    fn drain_returns_delta_once() {
        let h = handle("n", 48_000);
        for k in 0..3 {
            h.push_interleaved(&block(k, 10, 2), 10, 0);
        }
        let (start, out, ch) = h.drain();
        assert_eq!(start, 0);
        assert_eq!(ch, 2);
        let mut want = block(0, 10, 2);
        want.extend(block(1, 10, 2));
        want.extend(block(2, 10, 2));
        assert_eq!(out, want);
        let (start2, out2, _) = h.drain();
        assert_eq!(start2, 30);
        assert!(out2.is_empty());
    }

    #[test]
    fn drain_skips_overwritten_frames() {
        let h = handle("n", 48_000);
        // Push 4000 frames (> ring), drain must return only the last
        // SCOPE_RING_FRAMES with the absolute start index.
        for k in 0..25 {
            h.push_interleaved(&block(k, 200, 1), 200, 0);
        }
        let (start, out, ch) = h.drain();
        assert_eq!(ch, 1);
        assert_eq!(start, 0);
        assert_eq!(out.len(), 5000);
    }

    #[test]
    fn channel_change_resets_counters() {
        let h = handle("n", 48_000);
        h.push_interleaved(&block(1, 10, 2), 10, 0);
        h.push_interleaved(&block(2, 10, 1), 10, 0);
        let (_, ch) = h.snapshot();
        assert_eq!(ch, 1);
        let (start, out, _) = h.drain();
        assert_eq!(start, 0);
        assert_eq!(out, block(2, 10, 1));
    }

    #[test]
    fn push_interleaved_seeds_absolute_frames() {
        let h = WaveformHandle::for_recorder("n".to_string(), 48_000, 1000);
        h.push_interleaved(&block(1, 10, 2), 10, 1000);
        let (start, out, _) = h.drain();
        assert_eq!(start, 1000);
        assert_eq!(out, block(1, 10, 2));
    }

    #[test]
    fn zero_frames_is_noop() {
        let h = handle("n", 48_000);
        h.push_interleaved(&[], 0, 0);
        let (start, out, _) = h.drain();
        assert_eq!(start, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn channels_clamped_to_max() {
        let h = handle("n", 48_000);
        // 100-channel block → stride clamps to 64.
        h.push_interleaved(&block(1, 4, 100), 4, 0);
        let (_, ch) = h.snapshot();
        assert_eq!(ch, MAX_WAVEFORM_CHANNELS);
    }

    #[test]
    fn effect_process_captures_via_try_lock() {
        let d = WaveformData {};
        let (mut effect, h) = WaveformEffect::new(d, "n".to_string(), 48_000);
        let b = block(7, 4, 2);
        let mut buf = b.clone();
        effect.process(&mut buf, 4);
        let (out, ch) = h.snapshot();
        assert_eq!(ch, 2);
        assert_eq!(&out[out.len() - 8..], b.as_slice());
    }

    #[test]
    fn spectrum_handle_uses_long_window() {
        let (mut effect, h) = WaveformEffect::new_for("n".to_string(), 48_000);
        assert!(h.is_spectrum());
        // Push more than SPECTRUM_FRAMES; snapshot is capped at the window.
        for k in 0..3 {
            let b = block(k, 2000, 2);
            effect.process(&mut b.clone(), 2000);
        }
        let (out, ch) = h.snapshot();
        assert_eq!(ch, 2);
        assert_eq!(out.len(), SPECTRUM_FRAMES * 2);
    }

    proptest::proptest! {
        #[test]
        fn ring_roundtrip_preserves_last_written_frames(
            seed in 0u64..100_000,
            chunks in 1usize..8,
            ch in 1usize..5,
        ) {
            let h = handle("p", 48_000);
            let mut x = seed | 1;
            let mut expected: Vec<f64> = Vec::new();
            for k in 0..chunks {
                let frames = 1 + ((k * 37 + (seed as usize)) % 500);
                let b: Vec<f32> = (0..frames * ch)
                     .map(|_i| {
                        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                        ((x >> 33) as u32 as f32 / u32::MAX as f32) * 2.0 - 1.0
                    })
                    .collect();
                expected.extend(b.iter().map(|v| *v as f64));
                h.push_interleaved(&b, frames, 0);
            }
            let (start, out, got_ch) = h.drain();
            prop_assert_eq!(got_ch, ch);
            prop_assert_eq!(out.len(), expected.len());
            prop_assert!(start == 0);
            for (got, want) in out.iter().zip(&expected) {
                prop_assert!((*got as f64 - *want).abs() < 1e-6);
            }
        }
    }
}
