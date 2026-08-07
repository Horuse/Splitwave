use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::LimiterData;

use super::util::{db_to_linear, load_f32, store_f32};
use super::{Effect, EffectControl};

/// Brick-wall limiter: input is delayed by `lookahead_frames`; gain envelope
/// reacts to the upcoming peak so reduction lands before the peak emerges.
/// Instant attack, exponential release.
pub struct LimiterEffect {
    ceiling: Arc<AtomicU32>,
    release_ms: Arc<AtomicU32>,
    sample_rate: u32,
    lookahead_frames: usize,
    /// Stereo-interleaved look-ahead delay; both channels share `current_gain`.
    delay_buf: Box<[f32]>,
    delay_pos: usize,
    /// Per-frame max(|L|, |R|) over the same window as `delay_buf`. Peak in
    /// the window = `peak_buf.iter().max()`.
    peak_buf: Box<[f32]>,
    current_gain: f32,
    /// Min gain during the last block (1.0 = no limiting).
    pub gr_lin: Arc<AtomicU32>,
}

impl LimiterEffect {
    pub fn new(d: LimiterData, sample_rate: u32) -> (Self, EffectControl, Arc<AtomicU32>) {
        let lookahead_frames =
            ((d.lookahead_ms.max(0.1) * sample_rate as f32 / 1000.0) as usize).max(1);
        let ceiling_lin = db_to_linear(d.ceiling_db).max(1e-6);
        let ceiling = Arc::new(AtomicU32::new(ceiling_lin.to_bits()));
        let release_ms = Arc::new(AtomicU32::new(d.release_ms.max(0.1).to_bits()));
        let gr_lin = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let control = EffectControl::Limiter {
            ceiling: ceiling.clone(),
            release_ms: release_ms.clone(),
        };
        (
            Self {
                ceiling,
                release_ms,
                sample_rate,
                lookahead_frames,
                delay_buf: vec![0.0; lookahead_frames * 2].into_boxed_slice(),
                delay_pos: 0,
                peak_buf: vec![0.0; lookahead_frames].into_boxed_slice(),
                current_gain: 1.0,
                gr_lin: gr_lin.clone(),
            },
            control,
            gr_lin,
        )
    }

    pub fn from_state(
        ceiling: Arc<AtomicU32>,
        release_ms: Arc<AtomicU32>,
        lookahead_frames: usize,
        sample_rate: u32,
        gr_lin: Arc<AtomicU32>,
    ) -> Self {
        Self {
            ceiling,
            release_ms,
            sample_rate,
            lookahead_frames,
            delay_buf: vec![0.0; lookahead_frames * 2].into_boxed_slice(),
            delay_pos: 0,
            peak_buf: vec![0.0; lookahead_frames].into_boxed_slice(),
            current_gain: 1.0,
            gr_lin,
        }
    }
}

impl Effect for LimiterEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let ceiling = load_f32(&self.ceiling).max(1e-6);
        let release_ms = load_f32(&self.release_ms).max(0.1);
        let release_coeff = 1.0 - (-1.0 / (release_ms * 0.001 * self.sample_rate as f32)).exp();

        let lookahead = self.lookahead_frames;
        let stereo = &mut samples[..frames * 2];
        let mut block_min_gr = 1.0f32;
        for f in 0..frames {
            let l_in = stereo[f * 2];
            let r_in = stereo[f * 2 + 1];

            // Read the emerging (oldest) sample, then overwrite that slot.
            let l_out = self.delay_buf[self.delay_pos * 2];
            let r_out = self.delay_buf[self.delay_pos * 2 + 1];
            self.delay_buf[self.delay_pos * 2] = l_in;
            self.delay_buf[self.delay_pos * 2 + 1] = r_in;
            self.peak_buf[self.delay_pos] = l_in.abs().max(r_in.abs());
            self.delay_pos = if self.delay_pos + 1 == lookahead {
                0
            } else {
                self.delay_pos + 1
            };

            let mut peak = 0.0_f32;
            for &p in self.peak_buf.iter() {
                if p > peak {
                    peak = p;
                }
            }
            let target = if peak > ceiling { ceiling / peak } else { 1.0 };
            if target < self.current_gain {
                self.current_gain = target;
            } else {
                self.current_gain += (target - self.current_gain) * release_coeff;
            }

            if self.current_gain < block_min_gr {
                block_min_gr = self.current_gain;
            }
            stereo[f * 2] = l_out * self.current_gain;
            stereo[f * 2 + 1] = r_out * self.current_gain;
        }
        store_f32(&self.gr_lin, block_min_gr);
    }

    fn latency_frames(&self) -> usize {
        self.lookahead_frames
    }
}
