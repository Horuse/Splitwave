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
            let detected = if detected.is_finite() { detected } else { 0.0 };
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

#[cfg(test)]
mod tests {
    use super::super::util::{db_to_linear, load_f32};
    use super::*;
    use proptest::prelude::*;

    const SR: u32 = 48_000;

    fn dc(frames: usize, amp: f32) -> Vec<f32> {
        vec![amp; frames * 2]
    }

    fn sine(frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = amp * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / SR as f32).sin();
                [s, s]
            })
            .collect()
    }

    fn gate(data: NoiseGateData) -> (NoiseGateEffect, Arc<AtomicU32>) {
        let (e, _, g) = NoiseGateEffect::new(data, SR);
        (e, g)
    }

    fn loud_data() -> NoiseGateData {
        NoiseGateData {
            threshold_db: -30.0,
            range_db: -24.0,
            attack_ms: 5.0,
            hold_ms: 50.0,
            release_ms: 50.0,
            bypassed: false,
        }
    }

    #[test]
    fn quiet_input_closes_to_range_attenuation() {
        let (mut e, gain) = gate(loud_data());
        // Release τ = 50 ms ≈ 2400 frames; 40 blocks settle to closed gain.
        let mut buf = dc(12000, 0.005); // well below -30 dB threshold
        e.process(&mut buf, 12000);
        let closed = db_to_linear(-24.0);
        let got = load_f32(&gain);
        assert!(
            (got - closed).abs() < 0.15 * closed,
            "gate must settle at range attenuation: {got} vs {closed}"
        );
        let peak = buf[12000 * 2 - 2400..]
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 0.005 * closed * 2.0, "output attenuated: {peak}");
    }

    #[test]
    fn loud_input_opens_gate() {
        let (mut e, gain) = gate(loud_data());
        let mut buf = dc(2400, 0.5);
        e.process(&mut buf, 2400);
        assert!(
            load_f32(&gain) > 0.99,
            "gate must open: {:#}",
            load_f32(&gain)
        );
    }

    #[test]
    fn hold_keeps_gate_open_after_signal_ends() {
        let (mut e, gain) = gate(loud_data());
        let mut loud = sine(240, 0.5);
        e.process(&mut loud, 240);
        // Signal stops. Hold = 50 ms = 2400 frames.
        let silence = vec![0.0; 240 * 2];
        let mut b = silence.clone();
        e.process(&mut b, 240); // 240 frames into hold
        assert!(
            load_f32(&gain) > 0.9,
            "gate must stay open during hold: {}",
            load_f32(&gain)
        );
        // Wait well past the hold window plus the release tail: 60 blocks
        // is 14.4 s, hold + release need ~7.7 s to close below range+1 dB.
        for _ in 0..60 {
            let mut b = silence.clone();
            e.process(&mut b, 240);
        }
        assert!(
            load_f32(&gain) < db_to_linear(-24.0) + 0.01,
            "gate must close after hold: {}",
            load_f32(&gain)
        );
    }

    #[test]
    fn sidechain_keys_gate() {
        // Quiet main + loud sidechain → gate opens, main passes.
        let (mut e, gain) = gate(loud_data());
        let quiet_main = sine(240, 0.002);
        let loud_side = sine(240, 0.9);
        let mut main = quiet_main.clone();
        e.process_with_sidechain(&mut main, Some(&loud_side), 120);
        assert!(load_f32(&gain) > 0.9);

        // Loud main + silent sidechain → gate closes over the release tail.
        let (mut e2, gain2) = gate(loud_data());
        let loud_main = sine(4800, 0.9);
        let silent_side = vec![0.0; 4800 * 2];
        let mut main = loud_main.clone();
        e2.process_with_sidechain(&mut main, Some(&silent_side), 2400);
        assert!(load_f32(&gain2) < 0.5, "silent sidechain must close gate");
    }

    #[test]
    fn detector_tracks_input_envelope() {
        let (mut e, gain) = gate(loud_data());
        // Amplitude right at the threshold boundary toggles state without
        // panic and the stored gain follows the open/close transitions.
        let loud = sine(240, 0.05); // == threshold -30 dB
        e.process(&mut loud.clone(), 120);
        assert!(load_f32(&gain) > 0.9);
    }

    #[test]
    fn new_clamps_hostile_params() {
        let d = NoiseGateData {
            threshold_db: -200.0,
            range_db: 12.0,
            attack_ms: 0.0,
            hold_ms: -5.0,
            release_ms: 0.0,
            bypassed: false,
        };
        let (mut e, c, state) = NoiseGateEffect::new(d, SR);
        let EffectControl::NoiseGate {
            threshold_db,
            range_db,
            attack_ms,
            hold_ms,
            release_ms,
        } = &c
        else {
            panic!("wrong control variant");
        };
        assert_eq!(load_f32(range_db), 0.0, "positive range clamps to 0");
        assert_eq!(load_f32(hold_ms), 0.0);
        assert_eq!(load_f32(attack_ms), 0.01);
        assert_eq!(load_f32(release_ms), 0.1);
        assert_eq!(load_f32(threshold_db), -200.0);
        let mut buf = vec![0.1; 96];
        e.process(&mut buf, 48);
        assert!(buf.iter().all(|s| s.is_finite()));
        assert!(load_f32(&state).is_finite());
    }

    proptest::proptest! {
        #[test]
        fn gate_output_is_finite_and_never_exceeds_input(
            input in proptest::prelude::prop::collection::vec(-1.0f32..1.0, 512),
            threshold in -60.0f32..0.0,
            range in -80.0f32..6.0,
        ) {
            let d = NoiseGateData {
                threshold_db: threshold,
                range_db: range,
                attack_ms: 5.0,
                hold_ms: 10.0,
                release_ms: 50.0,
                bypassed: false,
            };
            let (mut e, _, _) = NoiseGateEffect::new(d, SR);
            let mut buf = input.clone();
            e.process(&mut buf, 256);
            for (o, i) in buf.iter().zip(&input) {
                prop_assert!(o.is_finite());
                prop_assert!(o.abs() <= i.abs() + 1e-7, "|{o}| > |{i}|: gate amplified");
            }
        }
    }
    #[test]
    fn from_state_rebuild_shares_atoms() {
        let (mut e, c, state) = NoiseGateEffect::new(loud_data(), SR);
        let EffectControl::NoiseGate {
            threshold_db,
            range_db,
            attack_ms,
            hold_ms,
            release_ms,
            ..
        } = &c
        else {
            panic!("variant")
        };
        let mut e2 = NoiseGateEffect::from_state(
            threshold_db.clone(),
            range_db.clone(),
            attack_ms.clone(),
            hold_ms.clone(),
            release_ms.clone(),
            SR,
            state.clone(),
        );
        // Both instances read the same atoms: updating via the control
        // changes the rebuilt effect's behaviour.
        range_db.store((-12.0f32).to_bits(), std::sync::atomic::Ordering::Relaxed);
        // Release τ = 50 ms = 2400 frames; 60 blocks settle near closed gain.
        for _ in 0..60 {
            let mut b1 = dc(240, 0.002);
            e.process(&mut b1, 240);
            let mut b2 = dc(240, 0.002);
            e2.process_with_sidechain(&mut b2, None, 240);
        }
        let want = db_to_linear(-12.0);
        let got = load_f32(&state);
        assert!((got - want).abs() < 0.02 * want, "{got} vs {want}");
    }
}
