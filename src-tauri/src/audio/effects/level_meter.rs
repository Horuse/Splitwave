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
