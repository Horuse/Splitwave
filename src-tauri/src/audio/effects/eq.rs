use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::EqData;

use super::biquad::{biquad_peaking, Biquad};
use super::util::load_f32;
use super::{Effect, EffectControl};

/// Center frequencies for the 10 ISO 1-octave bands.
pub const EQ_FREQUENCIES_HZ: [f32; 10] = [
    32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Q factor for 1-octave bandwidth (BW = 1 octave -> Q = 1 / (2 * sinh(ln(2)/2)) ≈ 1.4142).
pub const EQ_Q: f32 = 1.4142135;

/// 10-band graphic equalizer using cascaded second-order peaking biquads.
/// Cascading peaking filters guarantees exact phase coherence and magnitude flatness
/// (identity pass-through at 0 dB gain across all bands) without the destructive
/// inter-band phase cancellations of crossover ladders.
pub struct EqEffect {
    filters: [[Biquad; 10]; 2],
    last_gains_db: [f32; 10],
    gains: [Arc<AtomicU32>; 10],
    sample_rate: u32,
}

impl EqEffect {
    pub fn new(d: EqData, sample_rate: u32) -> (Self, EffectControl) {
        let gains: [Arc<AtomicU32>; 10] =
            std::array::from_fn(|i| Arc::new(AtomicU32::new(d.gains_db[i].to_bits())));
        let control = EffectControl::Eq {
            gains: gains.clone(),
        };
        let mut effect = Self {
            filters: [[Biquad::identity(); 10]; 2],
            last_gains_db: [0.0; 10],
            gains,
            sample_rate,
        };
        effect.update_coefficients(d.gains_db);
        (effect, control)
    }

    pub fn from_state(gains: [Arc<AtomicU32>; 10], sample_rate: u32) -> Self {
        let initial_gains = std::array::from_fn(|i| load_f32(&gains[i]));
        let mut effect = Self {
            filters: [[Biquad::identity(); 10]; 2],
            last_gains_db: [0.0; 10],
            gains,
            sample_rate,
        };
        effect.update_coefficients(initial_gains);
        effect
    }

    #[inline]
    fn update_coefficients(&mut self, gains_db: [f32; 10]) {
        for i in 0..10 {
            let gain = gains_db[i];
            if (gain - self.last_gains_db[i]).abs() > 1e-4 {
                let coeff = biquad_peaking(EQ_FREQUENCIES_HZ[i], EQ_Q, gain, self.sample_rate);
                self.filters[0][i].retune(coeff);
                self.filters[1][i].retune(coeff);
                self.last_gains_db[i] = gain;
            }
        }
    }
}

impl Effect for EqEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let current_gains = std::array::from_fn(|i| load_f32(&self.gains[i]));
        self.update_coefficients(current_gains);

        let stereo = &mut samples[..frames * 2];
        for frame in stereo.chunks_exact_mut(2) {
            let mut l = frame[0];
            let mut r = frame[1];
            for i in 0..10 {
                l = self.filters[0][i].process(l);
                r = self.filters[1][i].process(r);
            }
            frame[0] = l;
            frame[1] = r;
        }
    }
}
