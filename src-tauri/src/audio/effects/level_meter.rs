use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
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
    /// Per channel, the squared sum and the frames metered since the last
    /// tick, packed so the tick takes both at once: RMS covers the time
    /// between ticks, whatever the block size.
    window: Arc<Vec<AtomicU64>>,
}

/// Frames a window may hold before it starts over, should no tick come.
const MAX_WINDOW_FRAMES: u32 = 1 << 30;

fn pack(sum_sq: f32, frames: u32) -> u64 {
    (sum_sq.to_bits() as u64) << 32 | frames as u64
}

fn unpack(window: u64) -> (f32, u32) {
    (f32::from_bits((window >> 32) as u32), window as u32)
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
            window: Arc::new((0..MAX_METER_CHANNELS).map(|_| AtomicU64::new(0)).collect()),
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
            let decayed = self.peaks[c].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |b| {
                Some((f32::from_bits(b) * METER_PEAK_DECAY).to_bits())
            });
            let p = f32::from_bits(decayed.unwrap_or_else(|b| b));
            peaks.push(p);
            let (sum, frames) = unpack(self.window[c].swap(0, Ordering::Relaxed));
            // Nothing metered since the last tick keeps the last reading.
            if frames > 0 {
                store_f32(&self.rms[c], (sum / frames as f32).sqrt());
            }
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

/// Meter `channels`-wide interleaved f32. Peaks (max) and squared sums
/// accumulate since the last tick. RT-safe: fixed stack scratch, no allocation.
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
    // The tick decays and resets these at the same time, so each update is
    // one atomic read-modify-write: a load and a later store would undo it.
    for c in 0..channels {
        // Non-negative floats order the same as their bits.
        handle.peaks[c].fetch_max(peak[c].to_bits(), Ordering::Relaxed);
        let _ = handle.window[c].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |w| {
            let (sum, n) = unpack(w);
            let block = frames as u32;
            let add = sum_sq[c] as f32;
            Some(if n > MAX_WINDOW_FRAMES {
                pack(add, block)
            } else {
                pack(sum + add, n + block)
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tick_racing_the_audio_thread_never_reads_high() {
        let h = MeterHandle::new("n".to_string());
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let audio = {
            let (h, stop) = (h.clone(), stop.clone());
            std::thread::spawn(move || {
                let block = vec![0.5f32; 64];
                while !stop.load(Ordering::Relaxed) {
                    update_meter(&h, &block, 2);
                }
            })
        };
        for _ in 0..200_000 {
            let s = h.snapshot_and_decay();
            for (rms, peak) in s.rms.iter().zip(&s.peaks) {
                assert!(*rms <= 0.5 + 1e-4, "RMS {rms} of a 0.5 signal");
                assert!(*peak <= 0.5, "peak {peak} of a 0.5 signal");
            }
        }
        stop.store(true, Ordering::Relaxed);
        audio.join().unwrap();
    }

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
    }

    #[test]
    fn rms_covers_the_time_since_the_last_tick() {
        let h = MeterHandle::new("n".to_string());
        // Half the tick at 0.8, half silent: whatever the block size.
        for block in [32, 2048] {
            for _ in 0..4096 / block {
                update_meter(&h, &interleaved(2, block, |_, _| 0.8), 2);
            }
            for _ in 0..4096 / block {
                update_meter(&h, &vec![0.0; 2 * block], 2);
            }
            let s = h.snapshot_and_decay();
            let want = (0.8f32 * 0.8 / 2.0).sqrt();
            assert!((s.rms[0] - want).abs() < 1e-5, "{block}: {}", s.rms[0]);
        }
        // The next tick starts afresh.
        update_meter(&h, &vec![0.0; 480], 2);
        assert_eq!(h.snapshot_and_decay().rms[0], 0.0);
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
        // Both feed one reading: peaks keep the max, RMS spans both blocks.
        e.process(&mut interleaved(2, 240, |_, _| 0.4), 240);
        let s = h.snapshot_and_decay();
        assert_eq!(s.peaks[0], 0.6);
        let want = ((0.36f32 + 0.16) / 2.0).sqrt();
        assert!((s.rms[0] - want).abs() < 1e-6, "{}", s.rms[0]);
    }
}
