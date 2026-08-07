use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::NoiseGateData;

use super::util::{db_to_linear, load_f32, store_f32};
use super::{Effect, EffectControl};

/// Noise gate: closes (attenuates by `range_db`) when input falls below
/// `threshold_db`; a `hold_ms` timer prevents chatter on borderline signals.
pub struct NoiseGateEffect {
    threshold_db: Arc<AtomicU32>,
    range_db: Arc<AtomicU32>,
    attack_ms: Arc<AtomicU32>,
    hold_ms: Arc<AtomicU32>,
    release_ms: Arc<AtomicU32>,
    sample_rate: u32,
    envelope: f32,
    current_gain: f32,
    /// Frames remaining in the "open during hold" state; reset whenever the
    /// envelope crosses above threshold.
    hold_remaining: u32,
    /// Current gate gain (0-1 linear) written each block. 1.0 = fully open.
    pub state_gain: Arc<AtomicU32>,
}

/// Envelope-detector release time constant; short enough for fast gate
/// closing without re-opening on every transient.
const GATE_DETECTOR_RELEASE_MS: f32 = 10.0;

impl NoiseGateEffect {
    pub fn new(d: NoiseGateData, sample_rate: u32) -> (Self, EffectControl, Arc<AtomicU32>) {
        let threshold_db = Arc::new(AtomicU32::new(d.threshold_db.to_bits()));
        let range_db = Arc::new(AtomicU32::new(d.range_db.min(0.0).to_bits()));
        let attack_ms = Arc::new(AtomicU32::new(d.attack_ms.max(0.01).to_bits()));
        let hold_ms = Arc::new(AtomicU32::new(d.hold_ms.max(0.0).to_bits()));
        let release_ms = Arc::new(AtomicU32::new(d.release_ms.max(0.1).to_bits()));
        let state_gain = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let control = EffectControl::NoiseGate {
            threshold_db: threshold_db.clone(),
            range_db: range_db.clone(),
            attack_ms: attack_ms.clone(),
            hold_ms: hold_ms.clone(),
            release_ms: release_ms.clone(),
        };
        (
            Self {
                threshold_db,
                range_db,
                attack_ms,
                hold_ms,
                release_ms,
                sample_rate,
                envelope: 0.0,
                current_gain: 1.0,
                hold_remaining: 0,
                state_gain: state_gain.clone(),
            },
            control,
            state_gain,
        )
    }

    pub fn from_state(
        threshold_db: Arc<AtomicU32>,
        range_db: Arc<AtomicU32>,
        attack_ms: Arc<AtomicU32>,
        hold_ms: Arc<AtomicU32>,
        release_ms: Arc<AtomicU32>,
        sample_rate: u32,
        state_gain: Arc<AtomicU32>,
    ) -> Self {
        Self {
            threshold_db,
            range_db,
            attack_ms,
            hold_ms,
            release_ms,
            sample_rate,
            envelope: 0.0,
            current_gain: 1.0,
            hold_remaining: 0,
            state_gain,
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
        let range_db = load_f32(&self.range_db).min(0.0);
        let attack_ms = load_f32(&self.attack_ms).max(0.01);
        let hold_ms = load_f32(&self.hold_ms).max(0.0);
        let release_ms = load_f32(&self.release_ms).max(0.1);

        let sr = self.sample_rate as f32;
        let attack_coeff = 1.0 - (-1.0 / (attack_ms * 0.001 * sr)).exp();
        let release_coeff = 1.0 - (-1.0 / (release_ms * 0.001 * sr)).exp();
        let detector_release_coeff = 1.0 - (-1.0 / (GATE_DETECTOR_RELEASE_MS * 0.001 * sr)).exp();
        let threshold_lin = db_to_linear(threshold_db);
        let closed_gain = db_to_linear(range_db);
        let hold_samples = (hold_ms * 0.001 * sr) as u32;

        let main_buf = &mut main[..frames * 2];
        let side = sidechain.filter(|s| s.len() >= frames * 2);
        for (f, frame) in main_buf.chunks_exact_mut(2).enumerate() {
            let detected = match side {
                Some(s) => s[f * 2].abs().max(s[f * 2 + 1].abs()),
                None => frame[0].abs().max(frame[1].abs()),
            };
            let coeff = if detected > self.envelope {
                attack_coeff
            } else {
                detector_release_coeff
            };
            self.envelope += (detected - self.envelope) * coeff;

            let target_gain = if self.envelope >= threshold_lin {
                self.hold_remaining = hold_samples;
                1.0
            } else if self.hold_remaining > 0 {
                self.hold_remaining -= 1;
                1.0
            } else {
                closed_gain
            };

            let coeff = if target_gain > self.current_gain {
                attack_coeff
            } else {
                release_coeff
            };
            self.current_gain += (target_gain - self.current_gain) * coeff;

            frame[0] *= self.current_gain;
            frame[1] *= self.current_gain;
        }
        store_f32(&self.state_gain, self.current_gain);
    }
}

impl Effect for NoiseGateEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        self.process_inner(samples, None, frames);
    }
}
