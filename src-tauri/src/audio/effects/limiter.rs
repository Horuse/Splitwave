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

#[cfg(test)]
mod tests {
    use super::super::util::load_f32;
    use super::*;
    use proptest::prelude::*;

    const SR: u32 = 48_000;

    fn dc(frames: usize, amp: f32) -> Vec<f32> {
        vec![amp; frames * 2]
    }

    fn sine(frames: usize, amp: f32, freq: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR as f32).sin();
                [s, s]
            })
            .collect()
    }

    #[test]
    fn below_ceiling_is_exact_delayed_passthrough() {
        let d = LimiterData {
            ceiling_db: -6.0,
            lookahead_ms: 2.0, // 96 frames @ 48k
            release_ms: 50.0,
            bypassed: false,
        };
        let (mut e, _, _) = LimiterEffect::new(d, SR);
        let input = sine(240, 0.1, 1000.0);
        let mut buf = input.clone();
        e.process(&mut buf, 240);
        // First `lookahead` frames are the zero-initialised delay line.
        assert!(buf[..96 * 2].iter().all(|s| *s == 0.0));
        // Rest is the input, bit-exact, shifted by the lookahead.
        for i in 0..(240 - 96) * 2 {
            assert_eq!(buf[96 * 2 + i], input[i], "sample {i}");
        }
    }

    #[test]
    fn brickwall_never_exceeds_ceiling_after_lookahead() {
        let d = LimiterData {
            ceiling_db: -3.0,
            lookahead_ms: 1.0,
            release_ms: 50.0,
            bypassed: false,
        };
        let (mut e, _, _) = LimiterEffect::new(d, SR);
        let input = dc(1200, 1.0); // 6 dB over a -3 dBFS ceiling
        let mut buf = input.clone();
        e.process(&mut buf, 1200);
        let ceiling = db_to_linear(-3.0);
        for (i, s) in buf.iter().enumerate().skip(48 * 2) {
            assert!(s.abs() <= ceiling + 1e-5, "sample {i} exceeds ceiling: {s}");
        }
    }

    #[test]
    fn latency_equals_lookahead_frames() {
        let d = LimiterData {
            ceiling_db: -1.0,
            lookahead_ms: 3.0,
            release_ms: 50.0,
            bypassed: false,
        };
        let (e, _, _) = LimiterEffect::new(d, SR);
        assert_eq!(e.latency_frames(), 144);
        assert_eq!((3.0 * SR as f32 / 1000.0) as usize, 144);
    }

    #[test]
    fn gain_reduction_is_reported_when_limiting() {
        let d = LimiterData {
            ceiling_db: -12.0,
            lookahead_ms: 1.0,
            release_ms: 50.0,
            bypassed: false,
        };
        let (mut e, _, gr) = LimiterEffect::new(d, SR);
        let mut buf = sine(480, 1.0, 1000.0);
        e.process(&mut buf, 240);
        let gr_lin = load_f32(&gr);
        assert!(gr_lin < 1.0, "GR must report reduction, got {gr_lin}");
        // And it should be roughly ceiling / peak.
        let expected = db_to_linear(-12.0);
        assert!(
            (gr_lin - expected).abs() < 0.05 * expected,
            "gr {gr_lin} vs {expected}"
        );
    }

    #[test]
    fn release_recovers_after_loud_passage() {
        let d = LimiterData {
            ceiling_db: -3.0,
            lookahead_ms: 1.0,
            release_ms: 20.0,
            bypassed: false,
        };
        let (mut e, _, _) = LimiterEffect::new(d, SR);
        let mut loud = dc(240, 1.0); // 6 dB over the ceiling
        e.process(&mut loud, 240);
        // Loud burst gone: after the release tail the limiter must pass
        // quiet input through unattenuated.
        let quiet = dc(240, 0.05);
        let mut last = vec![0.0; 240 * 2];
        for _ in 0..30 {
            let mut b = quiet.clone();
            e.process(&mut b, 240);
            last.copy_from_slice(&b);
        }
        // Release τ = 20 ms ≈ 960 frames; 30 blocks leave ≈ e^-6 residual.
        let peak = last.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 0.048,
            "gain must recover to ~1.0 after release, peak {peak}"
        );
        assert!(peak < 0.05 * 1.01, "gain must not overshoot 1.0: {peak}");
    }

    #[test]
    fn new_clamps_hostile_params() {
        let d = LimiterData {
            ceiling_db: -200.0,
            lookahead_ms: 0.0,
            release_ms: 0.0,
            bypassed: false,
        };
        let (mut e, c, _) = LimiterEffect::new(d, SR);
        let EffectControl::Limiter {
            ceiling,
            release_ms,
        } = &c
        else {
            panic!("wrong control variant");
        };
        assert_eq!(load_f32(ceiling), db_to_linear(-60.0).max(1e-6));
        assert_eq!(load_f32(release_ms), 0.1);
        let mut buf = vec![0.5; 96];
        e.process(&mut buf, 48);
        assert!(buf.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn control_clamps_ceiling_and_release() {
        let d = LimiterData {
            ceiling_db: -6.0,
            lookahead_ms: 1.0,
            release_ms: 50.0,
            bypassed: false,
        };
        let (_, c, _) = LimiterEffect::new(d, SR);
        let EffectControl::Limiter {
            ceiling,
            release_ms,
        } = &c
        else {
            panic!("wrong control variant");
        };
        let mut update = serde_json::Map::new();
        update.insert("ceilingDb".into(), serde_json::json!(-120.0));
        update.insert("releaseMs".into(), serde_json::json!(0.0));
        c.apply_update(&serde_json::Value::Object(update));
        assert_eq!(load_f32(ceiling.as_ref()), 1e-6);
        assert_eq!(load_f32(release_ms.as_ref()), 0.1);
    }

    proptest::proptest! {
        #[test]
        fn limiter_output_is_finite_and_bounded(
            input in proptest::prelude::prop::collection::vec(-2.0f32..2.0, 512),
            ceiling_db in -30.0f32..6.0,
        ) {
            let d = LimiterData {
                ceiling_db,
                lookahead_ms: 1.0,
                release_ms: 50.0,
                bypassed: false,
            };
            let (mut e, _, _) = LimiterEffect::new(d, SR);
            let mut buf = input.clone();
            e.process(&mut buf, 256);
            let ceiling = db_to_linear(ceiling_db).max(1e-6);
            for s in buf.iter().skip(48 * 2) {
                prop_assert!(s.is_finite());
                prop_assert!(s.abs() <= ceiling + 1e-4, "|{s}| > {ceiling}");
            }
            let _ = input;
        }
    }
}
