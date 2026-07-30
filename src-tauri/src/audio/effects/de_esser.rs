use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::DeEsserData;

use super::biquad::{biquad_for, BandShape, Biquad};
use super::util::{db_to_linear, load_f32};
use super::{Effect, EffectControl};

const BUTTER_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;
const ATTACK_MS: f32 = 1.0;
const RELEASE_MS: f32 = 60.0;

/// Split-band de-esser. The signal is split at `frequency` (LR4); the high band
/// carries the sibilance, and a compressor acting only on that band pulls down
/// "s"/"sh" bursts. The low/mid body of the voice is summed back untouched, so
/// only the harsh band is tamed. No STFT — cheap and latency-free.
pub struct DeEsserEffect {
    frequency: Arc<AtomicU32>,
    threshold_db: Arc<AtomicU32>,
    ratio: Arc<AtomicU32>,
    sample_rate: u32,
    tuned_freq: f32,
    ch: [ChanState; 2],
}

struct ChanState {
    lp1: Biquad,
    lp2: Biquad,
    env: f32,
}

impl ChanState {
    fn new(freq: f32, sample_rate: u32) -> Self {
        let lp = biquad_for(BandShape::Lpf, freq, BUTTER_Q, sample_rate);
        Self {
            lp1: lp,
            lp2: lp,
            env: 0.0,
        }
    }
}

impl DeEsserEffect {
    pub fn new(d: DeEsserData, sample_rate: u32) -> (Self, EffectControl) {
        let freq = d.frequency.clamp(2000.0, 16000.0);
        let frequency = Arc::new(AtomicU32::new(freq.to_bits()));
        let threshold_db = Arc::new(AtomicU32::new(d.threshold_db.clamp(-80.0, 0.0).to_bits()));
        let ratio = Arc::new(AtomicU32::new(d.ratio.clamp(1.0, 12.0).to_bits()));
        let control = EffectControl::DeEsser {
            frequency: frequency.clone(),
            threshold_db: threshold_db.clone(),
            ratio: ratio.clone(),
        };
        (
            Self::build(frequency, threshold_db, ratio, sample_rate),
            control,
        )
    }

    pub fn from_state(
        frequency: Arc<AtomicU32>,
        threshold_db: Arc<AtomicU32>,
        ratio: Arc<AtomicU32>,
        sample_rate: u32,
    ) -> Self {
        Self::build(frequency, threshold_db, ratio, sample_rate)
    }

    fn build(
        frequency: Arc<AtomicU32>,
        threshold_db: Arc<AtomicU32>,
        ratio: Arc<AtomicU32>,
        sample_rate: u32,
    ) -> Self {
        let freq = load_f32(&frequency).clamp(2000.0, 16000.0);
        Self {
            frequency,
            threshold_db,
            ratio,
            sample_rate,
            tuned_freq: freq,
            ch: [
                ChanState::new(freq, sample_rate),
                ChanState::new(freq, sample_rate),
            ],
        }
    }
}

#[inline]
fn coeff(ms: f32, sr: f32) -> f32 {
    1.0 - (-1.0 / (ms * 0.001 * sr)).exp()
}

impl Effect for DeEsserEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let freq = load_f32(&self.frequency).clamp(2000.0, 16000.0);
        let threshold_db = load_f32(&self.threshold_db).clamp(-80.0, 0.0);
        let ratio = load_f32(&self.ratio).clamp(1.0, 12.0);

        let sr = self.sample_rate as f32;
        // Retune the crossover in place if the user moved it; keeps filter state.
        if (freq - self.tuned_freq).abs() > 0.5 {
            let c = biquad_for(BandShape::Lpf, freq, BUTTER_Q, self.sample_rate);
            for st in &mut self.ch {
                st.lp1.retune(c);
                st.lp2.retune(c);
            }
            self.tuned_freq = freq;
        }

        let attack = coeff(ATTACK_MS, sr);
        let release = coeff(RELEASE_MS, sr);
        let slope = 1.0 - 1.0 / ratio;

        for frame in samples[..frames * 2].chunks_exact_mut(2) {
            for c in 0..2 {
                let st = &mut self.ch[c];
                let x = frame[c];
                let low = st.lp2.process(st.lp1.process(x));
                let high = x - low;
                let ah = high.abs();

                let coeff = if ah > st.env { attack } else { release };
                st.env += (ah - st.env) * coeff;

                let env_db = 20.0 * (st.env + 1e-9).log10();
                let over = env_db - threshold_db;
                let gain = if over > 0.0 {
                    db_to_linear(-over * slope)
                } else {
                    1.0
                };

                frame[c] = low + gain * high;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn de_esser() -> DeEsserEffect {
        DeEsserEffect::new(
            DeEsserData {
                frequency: 6000.0,
                threshold_db: -30.0,
                ratio: 4.0,
                bypassed: false,
            },
            48_000,
        )
        .0
    }

    fn run(freq_rad: f32, amp: f32) -> (Vec<f32>, Vec<f32>) {
        let mut e = de_esser();
        let frames = 4000;
        let mut buf = vec![0.0f32; frames * 2];
        for f in 0..frames {
            let v = (f as f32 * freq_rad).sin() * amp;
            buf[f * 2] = v;
            buf[f * 2 + 1] = v;
        }
        let reference = buf.clone();
        e.process(&mut buf, frames);
        (buf, reference)
    }

    #[test]
    fn sibilant_band_is_reduced() {
        // ~9.5 kHz tone, above the 6 kHz crossover and above threshold.
        let (out, refr) = run(1.25, 0.5);
        let peak_out = out.iter().skip(2 * 3000).map(|v| v.abs()).fold(0.0, f32::max);
        let peak_ref = refr.iter().skip(2 * 3000).map(|v| v.abs()).fold(0.0, f32::max);
        assert!(peak_out < peak_ref * 0.7, "high band not reduced: {peak_out} vs {peak_ref}");
    }

    #[test]
    fn low_tone_untouched() {
        // ~150 Hz tone, well below the crossover.
        let (out, refr) = run(0.02, 0.5);
        for n in 3000..3990 {
            let d = (out[n * 2] - refr[n * 2]).abs();
            assert!(d < 0.02, "low tone altered at {n}: {d}");
        }
    }
}
