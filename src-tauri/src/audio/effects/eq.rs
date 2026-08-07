use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::EqData;

use super::biquad::{biquad_for, BandShape, Biquad};
use super::util::{db_to_linear, load_f32};
use super::{Effect, EffectControl};

/// Linkwitz-Riley 4th-order crossover points: geometric means between adjacent
/// band centres. LR4 = two cascaded 2nd-order Butterworth biquads; sum of
/// matched LPF/HPF at the same fc is allpass, so all 10 bands sum back to a
/// magnitude-flat output when their gains are unity.
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

/// Per-channel filter chain. The input cascades through 9 crossover splits:
/// each split peels off one band's slice via LPF and forwards the HPF residual
/// to the next stage. Band gains scale these slices and we sum.
struct ChannelChain {
    lpfs: [Lr4; 9],
    hpfs: [Lr4; 9],
}

impl ChannelChain {
    fn new(sample_rate: u32) -> Self {
        Self {
            lpfs: std::array::from_fn(|i| {
                Lr4::new(BandShape::Lpf, EQ_CROSSOVER_FREQS[i], sample_rate)
            }),
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
            let band = self.lpfs[i].process(residual);
            residual = self.hpfs[i].process(residual);
            sum += band * gains_linear[i];
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
