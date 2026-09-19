use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::EqData;

use super::biquad::{biquad_for, BandShape, Biquad};
use super::util::{db_to_linear, load_f32};
use super::{Effect, EffectControl};

/// Fourth-order crossover points at geometric means between adjacent bands.
const EQ_CROSSOVER_FREQS: [f32; 9] = [
    45.2548, 89.4427, 176.7767, 353.5534, 707.1068, 1414.2136, 2828.4271, 5656.8542, 11313.7085,
];

const BUTTER_Q: f32 = std::f32::consts::FRAC_1_SQRT_2; // 1/√2 ≈ 0.7071

/// Cascaded pair of Butterworth biquads — a 4th-order Linkwitz-Riley section.
#[derive(Clone, Copy, Default)]
struct Lr4 {
    a: Biquad,
    b: Biquad,
}

impl Lr4 {
    fn new(shape: BandShape, freq_hz: f32, sample_rate: u32) -> Self {
        let c = biquad_for(shape, freq_hz, BUTTER_Q, sample_rate);
        Lr4 { a: c, b: c }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.b.process(self.a.process(x))
    }
}

/// Per-channel filter chain. Each split derives its low band by subtracting the
/// high-pass residual, so unity gains reconstruct the input exactly.
struct ChannelChain {
    hpfs: [Lr4; 9],
}

impl ChannelChain {
    fn new(sample_rate: u32) -> Self {
        Self {
            hpfs: std::array::from_fn(|i| {
                Lr4::new(BandShape::Hpf, EQ_CROSSOVER_FREQS[i], sample_rate)
            }),
        }
    }

    #[inline]
    fn process(&mut self, x: f32, gains_linear: &[f32; 10]) -> f32 {
        let mut residual = x;
        let mut sum = 0.0;
        for i in 0..9 {
            let next = self.hpfs[i].process(residual);
            let band = residual - next;
            sum += band * gains_linear[i];
            residual = next;
        }
        sum + residual * gains_linear[9]
    }
}

pub struct EqEffect {
    channels: [ChannelChain; 2],
    gains: [Arc<AtomicU32>; 10],
}

impl EqEffect {
    pub fn new(d: EqData, sample_rate: u32) -> (Self, EffectControl) {
        let gains: [Arc<AtomicU32>; 10] =
            std::array::from_fn(|i| Arc::new(AtomicU32::new(d.gains_db[i].to_bits())));
        let control = EffectControl::Eq {
            gains: gains.clone(),
        };
        (
            Self {
                channels: [
                    ChannelChain::new(sample_rate),
                    ChannelChain::new(sample_rate),
                ],
                gains,
            },
            control,
        )
    }

    pub fn from_state(gains: [Arc<AtomicU32>; 10], sample_rate: u32) -> Self {
        Self {
            channels: [
                ChannelChain::new(sample_rate),
                ChannelChain::new(sample_rate),
            ],
            gains,
        }
    }
}

impl Effect for EqEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let gains_linear: [f32; 10] =
            std::array::from_fn(|i| db_to_linear(load_f32(&self.gains[i])));
        let stereo = &mut samples[..frames * 2];
        for frame in stereo.chunks_exact_mut(2) {
            frame[0] = self.channels[0].process(frame[0], &gains_linear);
            frame[1] = self.channels[1].process(frame[1], &gains_linear);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::util::{load_f32, store_f32};
    use super::*;

    const SR: u32 = 48_000;

    fn unity_data() -> EqData {
        EqData {
            gains_db: [0.0; 10],
            bypassed: false,
        }
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / buf.len().max(1) as f64)
            .sqrt() as f32
    }

    fn noise(frames: usize) -> Vec<f32> {
        // Deterministic LCG noise — no allocation-heavy RNG needed.
        let mut x = 0x2545F4914F6CDD1Du64;
        (0..frames)
            .flat_map(|_| {
                x = x
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let v = ((x >> 33) as u32 as f32 / u32::MAX as f32) * 2.0 - 1.0;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn unity_gains_reconstruct_every_sample() {
        let (mut e, _) = EqEffect::new(unity_data(), SR);
        let input = noise(9600);
        let mut buf = input.clone();
        e.process(&mut buf, 9600);
        let max_error = buf
            .iter()
            .zip(&input)
            .map(|(out, input)| (out - input).abs())
            .fold(0.0f32, f32::max);
        assert!(max_error < 1e-6, "unity reconstruction error: {max_error}");
    }

    #[test]
    fn all_bands_down_is_exact_silence() {
        let mut d = unity_data();
        d.gains_db = [-60.0; 10];
        let (mut e, _) = EqEffect::new(d, SR);
        let mut buf = noise(240);
        e.process(&mut buf, 240);
        assert!(buf.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn boosting_only_high_band_passes_high_not_low() {
        let mut d = unity_data();
        // Band 5 (707–1414 Hz) centred at 1 kHz; every other band is -60 dB
        // (== 0 linear).
        d.gains_db = [
            -60.0, -60.0, -60.0, -60.0, -60.0, 12.0, -60.0, -60.0, -60.0, -60.0,
        ];
        let (mut e, _) = EqEffect::new(d, SR);
        let sine: Vec<f32> = (0..2400)
            .flat_map(|i| {
                let s = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SR as f32).sin();
                [s, s]
            })
            .collect();
        let mut buf = sine.clone();
        e.process(&mut buf, 2400);
        // Compare steady-state tail (skip filter settle).
        let in_rms = rms(&sine[1200..]);
        let out_rms = rms(&buf[1200..]);
        assert!(
            out_rms > 2.0 * in_rms,
            "boosted band should pass its tone: {out_rms} vs {in_rms}"
        );

        // Same EQ on DC (lives in band 0) must be nearly silent.
        let (mut e, _) = EqEffect::new(d, SR);
        let mut low_buf = vec![0.5f32; 2400 * 2];
        e.process(&mut low_buf, 2400);
        let low_rms = rms(&low_buf[1200..]);
        assert!(
            low_rms < 0.05 * 0.5,
            "DC must be rejected by a highs-only EQ: {low_rms}"
        );
    }

    #[test]
    fn channels_are_processed_independently() {
        let (mut e, _) = EqEffect::new(unity_data(), SR);
        let mut buf = noise(480);
        buf.chunks_mut(2).for_each(|f| f[1] = 0.0);
        let l_before: Vec<f32> = buf.chunks_exact(2).map(|f| f[0]).collect();
        e.process(&mut buf, 240);
        assert!(
            buf.chunks_exact(2).all(|f| f[1] == 0.0),
            "silent channel must stay silent"
        );
        assert!(buf.iter().all(|s| s.is_finite()), "no NaN expected");
        // Left channel must have been processed at all (changed shape).
        assert!(
            buf.chunks_exact(2).zip(&l_before).any(|(f, l)| f[0] != *l),
            "left channel must actually pass through filters"
        );
    }

    #[test]
    fn from_state_shares_atoms_with_control() {
        let gains: [Arc<AtomicU32>; 10] =
            std::array::from_fn(|_| Arc::new(AtomicU32::new(0.0f32.to_bits())));
        let mut e = EqEffect::from_state(gains.clone(), SR);
        // DC lives in band 0, so boosting it +6 dB must double DC exactly.
        // Two cascaded LR4 sections at 45 Hz settle slowly → 6000 frames.
        store_f32(&gains[0], 6.02);
        let mut buf = vec![0.5f32; 6000 * 2];
        e.process(&mut buf, 6000);
        assert!(buf.iter().all(|s| s.is_finite()));
        let tail = &buf[6000 * 2 - 2000..];
        for s in tail {
            assert!((s - 1.0).abs() < 5e-3, "band 0 gain must apply to DC: {s}");
        }
    }

    #[test]
    fn control_applies_partial_gains_update() {
        let d = unity_data();
        let (_, c) = EqEffect::new(d, SR);
        let EffectControl::Eq { gains } = &c else {
            panic!("wrong control variant");
        };
        let mut update = serde_json::Map::new();
        update.insert("gainsDb".into(), serde_json::json!([1.0, 2.0, 3.0]));
        c.apply_update(&serde_json::Value::Object(update));
        assert_eq!(load_f32(&gains[0]), 1.0);
        assert_eq!(load_f32(&gains[1]), 2.0);
        assert_eq!(load_f32(&gains[2]), 3.0);
        assert_eq!(load_f32(&gains[9]), 0.0, "missing slots stay untouched");
        // Non-array value is ignored.
        let mut bad = serde_json::Map::new();
        bad.insert("gainsDb".into(), serde_json::json!("nope"));
        c.apply_update(&serde_json::Value::Object(bad));
        assert_eq!(load_f32(&gains[0]), 1.0);
    }
}

// Split into its own module so `use` noise stays out of the main tests.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    const SR: u32 = 48_000;

    fn noise(frames: usize, seed: u64) -> Vec<f32> {
        let mut x = seed | 1;
        (0..frames)
            .flat_map(|_| {
                x = x
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let v = ((x >> 33) as u32 as f32 / u32::MAX as f32) * 4.0 - 2.0;
                [v, v]
            })
            .collect()
    }

    proptest::proptest! {
        #[test]
        fn eq_output_is_finite_and_bounded(
            seed in 0u64..1_000_000,
            gains in proptest::prelude::prop::collection::vec(-24.0f32..24.0, 10),
            frames in proptest::prelude::prop_oneof![
                proptest::prelude::Just(1usize),
                proptest::prelude::Just(7usize),
                proptest::prelude::Just(64usize),
                proptest::prelude::Just(256usize),
            ],
        ) {
            let mut g = [0.0f32; 10];
            g.copy_from_slice(&gains);
            let d = EqData {
                gains_db: g,
                bypassed: false,
            };
            let (mut e, _) = EqEffect::new(d, SR);
            let input = noise(frames, seed);
            let mut buf = input.clone();
            e.process(&mut buf, frames);
            prop_assert!(buf.iter().all(|s| s.is_finite()));
            // ±24 dB per band, ten bands summed: theoretical worst case is
            // far below 1e3; a runaway filter would blow past it.
            prop_assert!(buf.iter().all(|s| s.abs() < 1e3));
            let _ = input;
        }
    }
}
