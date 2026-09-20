use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::CompressorData;

use super::util::{load_f32, store_f32};
use super::{Effect, EffectControl};

pub struct CompressorEffect {
    threshold_db: Arc<AtomicU32>,
    ratio: Arc<AtomicU32>,
    attack_ms: Arc<AtomicU32>,
    release_ms: Arc<AtomicU32>,
    knee_db: Arc<AtomicU32>,
    makeup_db: Arc<AtomicU32>,
    sample_rate: u32,
    envelope: f32,
    /// Min gain (0-1 linear, no makeup) across the last block. 1.0 = no GR.
    pub gr_lin: Arc<AtomicU32>,
}

impl CompressorEffect {
    pub fn new(d: CompressorData, sample_rate: u32) -> (Self, EffectControl, Arc<AtomicU32>) {
        let threshold_db = Arc::new(AtomicU32::new(d.threshold_db.to_bits()));
        let ratio = Arc::new(AtomicU32::new(d.ratio.max(1.0).to_bits()));
        let attack_ms = Arc::new(AtomicU32::new(d.attack_ms.max(0.01).to_bits()));
        let release_ms = Arc::new(AtomicU32::new(d.release_ms.max(0.1).to_bits()));
        let knee_db = Arc::new(AtomicU32::new(d.knee_db.max(0.0).to_bits()));
        let makeup_db = Arc::new(AtomicU32::new(d.makeup_db.to_bits()));
        let gr_lin = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let control = EffectControl::Compressor {
            threshold_db: threshold_db.clone(),
            ratio: ratio.clone(),
            attack_ms: attack_ms.clone(),
            release_ms: release_ms.clone(),
            knee_db: knee_db.clone(),
            makeup_db: makeup_db.clone(),
        };
        (
            Self {
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                knee_db,
                makeup_db,
                sample_rate,
                envelope: 0.0,
                gr_lin: gr_lin.clone(),
            },
            control,
            gr_lin,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_state(
        threshold_db: Arc<AtomicU32>,
        ratio: Arc<AtomicU32>,
        attack_ms: Arc<AtomicU32>,
        release_ms: Arc<AtomicU32>,
        knee_db: Arc<AtomicU32>,
        makeup_db: Arc<AtomicU32>,
        sample_rate: u32,
        gr_lin: Arc<AtomicU32>,
    ) -> Self {
        Self {
            threshold_db,
            ratio,
            attack_ms,
            release_ms,
            knee_db,
            makeup_db,
            sample_rate,
            envelope: 0.0,
            gr_lin,
        }
    }

    pub fn process_with_sidechain(
        &mut self,
        main: &mut [f32],
        sidechain: Option<&[f32]>,
        frames: usize,
    ) {
        self.process_inner(main, sidechain, frames);
    }

    fn process_inner(&mut self, main: &mut [f32], sidechain: Option<&[f32]>, frames: usize) {
        let threshold_db = load_f32(&self.threshold_db);
        let ratio = load_f32(&self.ratio).max(1.0);
        let attack_ms = load_f32(&self.attack_ms).max(0.01);
        let release_ms = load_f32(&self.release_ms).max(0.1);
        let knee_db = load_f32(&self.knee_db).max(0.0);
        let makeup_db = load_f32(&self.makeup_db);

        let sr = self.sample_rate as f32;
        let attack_coeff = 1.0 - (-1.0 / (attack_ms * 0.001 * sr)).exp();
        let release_coeff = 1.0 - (-1.0 / (release_ms * 0.001 * sr)).exp();
        let inv_ratio = 1.0 / ratio;
        let makeup_lin = 10f32.powf(makeup_db / 20.0);
        let half_knee = knee_db * 0.5;

        let main_buf = &mut main[..frames * 2];
        let side = sidechain.filter(|s| s.len() >= frames * 2);
        let mut block_min_gr = 1.0f32;
        for (f, frame) in main_buf.chunks_exact_mut(2).enumerate() {
            let detected = match side {
                Some(s) => s[f * 2].abs().max(s[f * 2 + 1].abs()),
                None => frame[0].abs().max(frame[1].abs()),
            };
            let detected = if detected.is_finite() { detected } else { 0.0 };
            if detected > self.envelope {
                self.envelope += (detected - self.envelope) * attack_coeff;
            } else {
                self.envelope += (detected - self.envelope) * release_coeff;
            }

            let env_db = if self.envelope < 1e-6 {
                -120.0
            } else {
                20.0 * self.envelope.log10()
            };
            let over = env_db - threshold_db;
            let gain_red_db = if knee_db > 0.0 && over > -half_knee && over < half_knee {
                let x = over + half_knee;
                (1.0 - inv_ratio) * x * x / (2.0 * knee_db)
            } else if over > 0.0 {
                over * (1.0 - inv_ratio)
            } else {
                0.0
            };
            let gr_only = 10f32.powf(-gain_red_db / 20.0);
            if gr_only < block_min_gr {
                block_min_gr = gr_only;
            }
            let gain = gr_only * makeup_lin;
            frame[0] = if frame[0].is_finite() {
                frame[0] * gain
            } else {
                0.0
            };
            frame[1] = if frame[1].is_finite() {
                frame[1] * gain
            } else {
                0.0
            };
        }
        store_f32(&self.gr_lin, block_min_gr);
    }
}

impl Effect for CompressorEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        self.process_inner(samples, None, frames);
    }
}
#[cfg(test)]
mod tests {
    use super::super::util::{db_to_linear, load_f32};
    use super::*;
    use proptest::prelude::*;

    const SR: u32 = 48_000;

    fn sine(frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = amp * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SR as f32).sin();
                [s, s]
            })
            .collect()
    }

    fn dc(frames: usize, amp: f32) -> Vec<f32> {
        vec![amp; frames * 2]
    }

    fn run(e: &mut CompressorEffect, data: &[f32], frames: usize) -> Vec<f32> {
        let mut buf = data.to_vec();
        e.process(&mut buf, frames);
        buf
    }

    #[test]
    fn silence_stays_silent_and_gr_unmoved() {
        let d = CompressorData {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            knee_db: 0.0,
            makeup_db: 6.0,
            bypassed: false,
        };
        let (mut e, _, gr) = CompressorEffect::new(d, SR);
        let out = run(&mut e, &dc(240, 0.0), 240);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(load_f32(&gr), 1.0);
    }

    #[test]
    fn below_threshold_is_bit_exact_passthrough() {
        let d = CompressorData {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        };
        let (mut e, _, _) = CompressorEffect::new(d, SR);
        let input = sine(240, 0.1);
        let out = run(&mut e, &input, 240);
        assert_eq!(out, input);
    }

    #[test]
    fn static_curve_matches_above_threshold() {
        // Constant 0.5 == -6.02 dBFS envelope; threshold -12 dB, ratio 4:
        // GR = over * (1 - 1/r) = 6.02 * 0.75 dB.
        let d = CompressorData {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        };
        let (mut e, _, gr) = CompressorEffect::new(d, SR);
        let out = run(&mut e, &dc(2400, 0.5), 2400);
        let expected = 10f32.powf(-(6.02 * 0.75) / 20.0);
        let got = load_f32(&gr);
        assert!(
            (got - expected).abs() < 0.01 * expected,
            "gr {got} vs {expected}"
        );
        for s in &out[2400 * 2 - 480..] {
            assert!((s - 0.5 * expected).abs() < 0.01 * 0.5, "out {s}");
        }
    }

    #[test]
    fn soft_knee_starts_below_threshold_hard_knee_does_not() {
        // env -12.6 dB → over -0.6 dB: inside a 6 dB knee the soft curve is
        // already compressing; the hard knee is still exact passthrough.
        let mk = |knee: f32| CompressorData {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            knee_db: knee,
            makeup_db: 0.0,
            bypassed: false,
        };
        let (mut eh, _, _) = CompressorEffect::new(mk(0.0), SR);
        let (mut es, _, _) = CompressorEffect::new(mk(6.0), SR);
        let oh = run(&mut eh, &dc(2400, 0.234), 2400);
        let os = run(&mut es, &dc(2400, 0.234), 2400);
        // Settled tail (attack 5 ms ≈ 240 frames @ 48k).
        let ph = oh[4798];
        let ps = os[4798];
        assert!(
            (ph - 0.234).abs() < 1e-4,
            "hard knee below threshold must pass through: {ph}"
        );
        assert!(
            ps < 0.234,
            "soft knee already reduces below threshold: {ps}"
        );
        assert!(ps > 0.234 * 0.9, "soft knee must not over-compress: {ps}");
    }

    #[test]
    fn makeup_gain_is_applied_below_threshold() {
        let d = CompressorData {
            threshold_db: 0.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            knee_db: 0.0,
            makeup_db: 6.02,
            bypassed: false,
        };
        let (mut e, _, _) = CompressorEffect::new(d, SR);
        let input = dc(240, 0.25);
        let out = run(&mut e, &input, 240);
        for (o, i) in out.iter().zip(&input) {
            let want = i * db_to_linear(6.02);
            assert!((o - want).abs() < 1e-4, "{o} vs {want}");
        }
    }

    #[test]
    fn sidechain_drives_detection_not_main() {
        let d = CompressorData {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        };
        let (mut e, _, _) = CompressorEffect::new(d, SR);
        let quiet_main = dc(2400, 0.05);
        let loud_side = dc(2400, 0.9);
        let mut main = quiet_main.clone();
        e.process_with_sidechain(&mut main, Some(&loud_side), 2400);
        let tail = &main[2400 * 2 - 480..];
        let peak = tail.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak < 0.05 * 0.6,
            "loud sidechain must compress quiet main: {peak}"
        );
        assert!(main.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn new_clamps_hostile_params() {
        let d = CompressorData {
            threshold_db: -120.0,
            ratio: 0.0,
            attack_ms: 0.0,
            release_ms: 0.0,
            knee_db: -3.0,
            makeup_db: 99.0,
            bypassed: false,
        };
        let (mut e, c, _) = CompressorEffect::new(d, SR);
        let EffectControl::Compressor {
            threshold_db,
            ratio,
            attack_ms,
            release_ms,
            knee_db,
            makeup_db,
        } = &c
        else {
            panic!("wrong control variant");
        };
        assert_eq!(load_f32(ratio), 1.0);
        assert_eq!(load_f32(attack_ms), 0.01);
        assert_eq!(load_f32(release_ms), 0.1);
        assert_eq!(load_f32(knee_db), 0.0);
        assert_eq!(load_f32(threshold_db), -120.0);
        assert_eq!(load_f32(makeup_db), 99.0);
        let out = run(&mut e, &dc(96, 0.5), 48);
        assert!(out.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn non_finite_input_is_silenced_and_state_recovers() {
        let d = CompressorData {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        };
        let (mut e, _, _) = CompressorEffect::new(d, SR);
        let mut input = dc(5, 0.5);
        input[4] = f32::INFINITY;
        let out = run(&mut e, &input, 5);
        assert_eq!(out[4], 0.0);
        assert!(out.iter().all(|s| s.is_finite()));
        let follow = sine(240, 0.2);
        let out2 = run(&mut e, &follow, 240);
        assert!(out2.iter().all(|s| s.is_finite() && !s.is_nan()));
        assert!(
            out2.iter().zip(&follow).all(|(o, i)| (o - i).abs() < 1e-4),
            "GR must return to unity after the spike"
        );
    }

    #[test]
    fn shared_gr_atom_updates_each_block() {
        let d = CompressorData {
            threshold_db: -30.0,
            ratio: 8.0,
            attack_ms: 1.0,
            release_ms: 100.0,
            knee_db: 0.0,
            makeup_db: 0.0,
            bypassed: false,
        };
        let (mut e, _, gr) = CompressorEffect::new(d, SR);
        let _ = run(&mut e, &dc(240, 0.5), 240);
        assert!(load_f32(&gr) < 1.0, "loud block must report GR");
        for _ in 0..80 {
            let _ = run(&mut e, &dc(240, 0.0), 240);
        }
        assert_eq!(load_f32(&gr), 1.0);
    }

    proptest! {
        #[test]
        fn never_amplifies_without_makeup(
            input in prop::collection::vec(-2.0f32..2.0, 512),
            threshold in -60.0f32..0.0,
            ratio in 1.0f32..20.0,
            knee in 0.0f32..12.0,
        ) {
            let d = CompressorData {
                threshold_db: threshold,
                ratio,
                attack_ms: 5.0,
                release_ms: 100.0,
                knee_db: knee,
                makeup_db: 0.0,
                bypassed: false,
            };
            let (mut e, _, _) = CompressorEffect::new(d, SR);
            let mut buf = input.clone();
            e.process(&mut buf, 256);
            for (o, i) in buf.iter().zip(&input) {
                prop_assert!(o.is_finite());
                prop_assert!(o.abs() <= i.abs() + 1e-6, "|{o}| > |{i}|");
            }
        }

        #[test]
        fn non_finite_input_never_escapes(
            input in prop::collection::vec(-2.0f32..2.0, 512),
            poison_pos in 0usize..512,
            makeup in -12.0f32..24.0,
        ) {
            let d = CompressorData {
                threshold_db: -12.0,
                ratio: 4.0,
                attack_ms: 5.0,
                release_ms: 100.0,
                knee_db: 0.0,
                makeup_db: makeup,
                bypassed: false,
            };
            let (mut e, _, _) = CompressorEffect::new(d, SR);
            let mut buf = input;
            buf[poison_pos] = f32::INFINITY;
            e.process(&mut buf, 256);
            for (k, s) in buf.iter().enumerate() {
                prop_assert!(s.is_finite(), "non-finite output at sample {k}");
                if k == poison_pos { prop_assert_eq!(*s, 0.0); }
            }
        }
    }
}
