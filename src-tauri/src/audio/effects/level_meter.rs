use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::audio::graph::LevelMeterData;

use super::util::{load_f32, store_f32};
use super::Effect;

pub struct LevelMeterEffect {
    handle: MeterHandle,
}

/// Upper bound on metered channels; sizes the fixed atomic arrays so metering
/// never allocates on the RT path.
pub const MAX_METER_CHANNELS: usize = 64;

#[derive(Clone)]
pub struct MeterHandle {
    pub node_id: String,
    channels: Arc<AtomicUsize>,
    peaks: Arc<Vec<AtomicU32>>,
    rms: Arc<Vec<AtomicU32>>,
}

#[derive(Debug, Clone)]
pub struct MeterSnapshot {
    pub peaks: Vec<f32>,
    pub rms: Vec<f32>,
}

/// Peak fall-off per tick — prevents transients from latching the meter.
pub const METER_PEAK_DECAY: f32 = 0.85;

impl MeterHandle {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            channels: Arc::new(AtomicUsize::new(0)),
            peaks: Arc::new((0..MAX_METER_CHANNELS).map(|_| AtomicU32::new(0)).collect()),
            rms: Arc::new((0..MAX_METER_CHANNELS).map(|_| AtomicU32::new(0)).collect()),
        }
    }

    /// Snapshot current values and decay the peaks — called from the engine's
    /// tick thread.
    pub fn snapshot_and_decay(&self) -> MeterSnapshot {
        let n = self
            .channels
            .load(Ordering::Relaxed)
            .min(MAX_METER_CHANNELS);
        let mut peaks = Vec::with_capacity(n);
        let mut rms = Vec::with_capacity(n);
        for c in 0..n {
            let p = load_f32(&self.peaks[c]);
            store_f32(&self.peaks[c], p * METER_PEAK_DECAY);
            peaks.push(p);
            rms.push(load_f32(&self.rms[c]));
        }
        MeterSnapshot { peaks, rms }
    }
}

impl LevelMeterEffect {
    pub fn new(_d: LevelMeterData, node_id: String) -> (Self, MeterHandle) {
        let handle = MeterHandle::new(node_id);
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }

    pub fn from_handle(handle: MeterHandle) -> Self {
        Self { handle }
    }
}

impl Effect for LevelMeterEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let channels = if frames == 0 {
            0
        } else {
            samples.len() / frames
        };
        update_meter(&self.handle, &samples[..frames * channels.max(1)], channels);
    }
}

/// Meter `channels`-wide interleaved f32. Peaks accumulate (max) since the last
/// tick; RMS is per-block. RT-safe: fixed stack scratch, no allocation.
pub fn update_meter(handle: &MeterHandle, interleaved: &[f32], channels: usize) {
    let channels = channels.min(MAX_METER_CHANNELS);
    if channels == 0 {
        return;
    }
    let frames = interleaved.len() / channels;
    if frames == 0 {
        return;
    }
    let mut peak = [0.0f32; MAX_METER_CHANNELS];
    let mut sum_sq = [0.0f64; MAX_METER_CHANNELS];
    for f in 0..frames {
        let base = f * channels;
        for c in 0..channels {
            let v = interleaved[base + c];
            let a = v.abs();
            if a > peak[c] {
                peak[c] = a;
            }
            sum_sq[c] += (v as f64) * (v as f64);
        }
    }
    handle.channels.store(channels, Ordering::Relaxed);
    for c in 0..channels {
        let existing = load_f32(&handle.peaks[c]);
        store_f32(&handle.peaks[c], existing.max(peak[c]));
        store_f32(&handle.rms[c], (sum_sq[c] / frames as f64).sqrt() as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interleaved(ch: usize, frames: usize, f: impl Fn(usize, usize) -> f32) -> Vec<f32> {
        (0..frames * ch).map(|i| f(i / ch, i % ch)).collect()
    }

    #[test]
    fn peak_and_rms_of_constant_block() {
        let h = MeterHandle::new("n".to_string());
        update_meter(&h, &interleaved(2, 240, |_, _| 0.5), 2);
        let s = h.snapshot_and_decay();
        assert_eq!(s.peaks.len(), 2);
        assert_eq!(s.rms.len(), 2);
        for p in &s.peaks {
            assert_eq!(*p, 0.5);
        }
        for r in &s.rms {
            assert_eq!(*r, 0.5);
        }
    }

    #[test]
    fn peak_is_max_across_blocks() {
        let h = MeterHandle::new("n".to_string());
        update_meter(&h, &interleaved(2, 240, |_, _| 0.1), 2);
        update_meter(
            &h,
            &interleaved(2, 240, |_, c| if c == 0 { 0.9 } else { 0.2 }),
            2,
        );
        let s = h.snapshot_and_decay();
        assert_eq!(s.peaks[0], 0.9);
        assert_eq!(s.peaks[1], 0.2);
        // RMS is per-block: reflects only the last block's content.
        assert_eq!(s.rms[0], 0.9);
        assert_eq!(s.rms[1], 0.2);
    }

    #[test]
    fn rms_is_per_block_not_accumulated() {
        let h = MeterHandle::new("n".to_string());
        update_meter(&h, &interleaved(2, 240, |_, _| 0.8), 2);
        update_meter(&h, &vec![0.0; 480], 2);
        let s = h.snapshot_and_decay();
        for r in &s.rms {
            assert_eq!(*r, 0.0);
        }
    }

    #[test]
    fn peaks_decay_per_tick() {
        let h = MeterHandle::new("n".to_string());
        update_meter(&h, &interleaved(2, 240, |_, _| 1.0), 2);
        let first = h.snapshot_and_decay();
        assert_eq!(first.peaks[0], 1.0);
        let second = h.snapshot_and_decay();
        assert!((second.peaks[0] - METER_PEAK_DECAY).abs() < 1e-6);
        let _ = first;
    }

    #[test]
    fn channels_clamped_and_zero_is_noop() {
        let h = MeterHandle::new("n".to_string());
        update_meter(&h, &interleaved(100, 4, |_, c| c as f32), 100);
        let s = h.snapshot_and_decay();
        assert_eq!(s.peaks.len(), MAX_METER_CHANNELS);
        // 0 channels → early return, nothing stored.
        let h2 = MeterHandle::new("n2".to_string());
        update_meter(&h2, &vec![0.0; 10], 0);
        let s2 = h2.snapshot_and_decay();
        assert!(s2.peaks.is_empty());
    }

    #[test]
    fn metering_does_not_modify_samples() {
        let h = MeterHandle::new("n".to_string());
        let mut buf = interleaved(2, 240, |f, c| (f as f32) * 0.01 + c as f32);
        let original = buf.clone();
        LevelMeterEffect::from_handle(h.clone()).process(&mut buf, 240);
        assert_eq!(buf, original);
    }

    #[test]
    fn handles_reused_across_instances_share_state() {
        let h = MeterHandle::new("n".to_string());
        update_meter(&h, &interleaved(2, 240, |_, _| 0.6), 2);
        let mut e = LevelMeterEffect::from_handle(h.clone());
        // Per-block RMS follows the newest block; peaks keep the max.
        e.process(&mut interleaved(2, 240, |_, _| 0.4), 240);
        let s = h.snapshot_and_decay();
        assert_eq!(s.peaks[0], 0.6);
        assert_eq!(s.rms[0], 0.4);
    }
}
