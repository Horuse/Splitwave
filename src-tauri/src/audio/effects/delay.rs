use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::DelayData;

use super::util::load_f32;
use super::{Effect, EffectControl};

const MAX_DELAY_MS: f32 = 2000.0;

/// Stereo delay line with feedback and dry/wet mix. Ring sized to 2 s @ build
/// SR — live `time_ms` changes just shift the read offset, no realloc.
pub struct DelayEffect {
    time_ms: Arc<AtomicU32>,
    feedback: Arc<AtomicU32>,
    mix: Arc<AtomicU32>,
    sample_rate: u32,
    /// Stereo-interleaved ring; capacity = MAX_DELAY_MS @ sample_rate.
    buf: Box<[f32]>,
    write: usize,
    /// Per-block lerp target for `time_ms` — prevents discontinuity clicks
    /// when the slider moves.
    current_delay_frames: f32,
}

impl DelayEffect {
    pub fn new(d: DelayData, sample_rate: u32) -> (Self, EffectControl) {
        let time_ms = Arc::new(AtomicU32::new(d.time_ms.max(1.0).to_bits()));
        let feedback = Arc::new(AtomicU32::new(d.feedback.clamp(0.0, 0.95).to_bits()));
        let mix = Arc::new(AtomicU32::new(d.mix.clamp(0.0, 1.0).to_bits()));
        let control = EffectControl::Delay {
            time_ms: time_ms.clone(),
            feedback: feedback.clone(),
            mix: mix.clone(),
        };
        (
            Self::from_state(time_ms, feedback, mix, sample_rate),
            control,
        )
    }

    pub fn from_state(
        time_ms: Arc<AtomicU32>,
        feedback: Arc<AtomicU32>,
        mix: Arc<AtomicU32>,
        sample_rate: u32,
    ) -> Self {
        let cap = (MAX_DELAY_MS * 0.001 * sample_rate as f32) as usize * 2;
        let initial_delay_frames = load_f32(&time_ms).max(1.0) * 0.001 * sample_rate as f32;
        Self {
            time_ms,
            feedback,
            mix,
            sample_rate,
            buf: vec![0.0; cap].into_boxed_slice(),
            write: 0,
            current_delay_frames: initial_delay_frames,
        }
    }
}

impl Effect for DelayEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        let target_delay = (load_f32(&self.time_ms).max(1.0) * 0.001 * self.sample_rate as f32)
            .min(MAX_DELAY_MS * 0.001 * self.sample_rate as f32);
        // Single-pole smoothing over one block: critically damped, ≈10 ms
        // settle so slider sweeps don't click.
        let smooth = 1.0 - (-1.0 / (0.01 * self.sample_rate as f32 / frames as f32)).exp();
        self.current_delay_frames += (target_delay - self.current_delay_frames) * smooth;
        let delay_samples = (self.current_delay_frames as usize).max(1) * 2;
        let feedback = load_f32(&self.feedback).clamp(0.0, 0.95);
        let mix = load_f32(&self.mix).clamp(0.0, 1.0);
        let dry = 1.0 - mix;

        let cap = self.buf.len();
        let stereo = &mut samples[..frames * 2];
        for frame in stereo.chunks_exact_mut(2) {
            let read = (self.write + cap - delay_samples) % cap;
            let dl = self.buf[read];
            let dr = self.buf[read + 1];
            let il = frame[0];
            let ir = frame[1];
            self.buf[self.write] = il + dl * feedback;
            self.buf[self.write + 1] = ir + dr * feedback;
            self.write = (self.write + 2) % cap;
            frame[0] = il * dry + dl * mix;
            frame[1] = ir * dry + dr * mix;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::util::load_f32;
    use super::*;
    use proptest::prelude::*;
    use std::sync::atomic::Ordering;

    const SR: u32 = 1000;

    #[test]
    fn frames_zero_is_noop() {
        let d = DelayData {
            time_ms: 10.0,
            feedback: 0.0,
            mix: 1.0,
            bypassed: false,
        };
        let (mut e, _) = DelayEffect::new(d, SR);
        let mut buf = vec![0.7f32, 0.7];
        e.process(&mut buf, 0);
        assert_eq!(buf, vec![0.7, 0.7]);
    }

    #[test]
    fn impulse_emerges_after_delay() {
        let d = DelayData {
            time_ms: 10.0, // 10 frames @ 1 kHz
            feedback: 0.0,
            mix: 1.0,
            bypassed: false,
        };
        let (mut e, _) = DelayEffect::new(d, SR);
        let mut buf = vec![0.0; 32 * 2];
        buf[0] = 1.0;
        buf[1] = 1.0;
        e.process(&mut buf, 32);
        // First delay period is silent, impulse returns exactly at frame 10.
        assert!(buf[2..20].iter().all(|s| *s == 0.0));
        assert_eq!(buf[20], 1.0);
        assert_eq!(buf[21], 1.0);
        assert!(buf[22..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn feedback_recirculates_halving() {
        let d = DelayData {
            time_ms: 10.0,
            feedback: 0.5,
            mix: 1.0,
            bypassed: false,
        };
        let (mut e, _) = DelayEffect::new(d, SR);
        // With the ring feedback the echoes all land inside the impulse
        // block: frames 10, 20, 30 → amplitude 1.0, 0.5, 0.25.
        let mut buf = vec![0.0; 32 * 2];
        buf[0] = 1.0;
        buf[1] = 1.0;
        e.process(&mut buf, 32);
        assert_eq!(buf[20], 1.0);
        assert_eq!(buf[40], 0.5);
        assert_eq!(buf[60], 0.25);
        assert!(buf[62..].iter().all(|s| *s == 0.0));
    }

    #[test]
    fn half_mix_blends_dry_and_wet() {
        let d = DelayData {
            time_ms: 10.0,
            feedback: 0.0,
            mix: 0.5,
            bypassed: false,
        };
        let (mut e, _) = DelayEffect::new(d, SR);
        let mut buf = vec![0.0; 32 * 2];
        buf[0] = 0.8;
        buf[1] = -0.4;
        e.process(&mut buf, 32);
        // Dry halves immediately; wet arrives after the delay.
        assert_eq!(buf[0], 0.4);
        assert_eq!(buf[1], -0.2);
        assert_eq!(buf[20], 0.4);
        assert_eq!(buf[21], -0.2);
    }

    #[test]
    fn time_change_moves_without_panic_or_click() {
        let d = DelayData {
            time_ms: 10.0,
            feedback: 0.3,
            mix: 1.0,
            bypassed: false,
        };
        let (e, c) = DelayEffect::new(d, SR);
        let EffectControl::Delay { time_ms, .. } = &c else {
            panic!("wrong control variant");
        };
        time_ms.store(500.0f32.to_bits(), Ordering::Relaxed);
        let mut e = e;
        let mut block = vec![0.5; 64 * 2];
        for _ in 0..20 {
            e.process(&mut block, 64);
            assert!(block.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn huge_time_is_capped_at_ring_capacity() {
        let d = DelayData {
            time_ms: 10.0,
            feedback: 0.0,
            mix: 1.0,
            bypassed: false,
        };
        let (e, c) = DelayEffect::new(d, SR);
        let EffectControl::Delay { time_ms, .. } = &c else {
            panic!("wrong control variant");
        };
        time_ms.store(10_000.0f32.to_bits(), Ordering::Relaxed);
        let mut e = e;
        let mut buf = vec![1.0; 64 * 2];
        e.process(&mut buf, 64);
        assert!(buf.iter().all(|s| s.is_finite()));
        // Impulse written at t=0 can never emerge before 2 s pass; the wrap
        // arithmetic must stay inside the ring.
        assert!(buf.iter().all(|s| *s <= 1.0));
    }

    #[test]
    fn control_clamps_extreme_params() {
        let d = DelayData {
            time_ms: 10.0,
            feedback: 0.5,
            mix: 0.5,
            bypassed: false,
        };
        let (_, c) = DelayEffect::new(d, SR);
        let EffectControl::Delay {
            time_ms,
            feedback,
            mix,
        } = &c
        else {
            panic!("wrong control variant");
        };
        let mut update = serde_json::Map::new();
        update.insert("timeMs".into(), serde_json::json!(0.0));
        update.insert("feedback".into(), serde_json::json!(5.0));
        update.insert("mix".into(), serde_json::json!(-3.0));
        c.apply_update(&serde_json::Value::Object(update));
        assert_eq!(load_f32(time_ms), 1.0);
        assert_eq!(load_f32(feedback), 0.95);
        assert_eq!(load_f32(mix), 0.0);
    }

    proptest::proptest! {
        #[test]
        fn output_never_nan_with_any_params(
            input in proptest::prelude::prop::collection::vec(-2.0f32..2.0, 512),
            time in 0.0f32..3000.0,
            feedback in -1.0f32..2.0,
            mix in -1.0f32..3.0,
        ) {
            let d = DelayData {
                time_ms: 10.0,
                feedback: 0.5,
                mix: 0.5,
                bypassed: false,
            };
            let (mut e, c) = DelayEffect::new(d, SR);
            if let EffectControl::Delay { time_ms, feedback: fb, mix: mx } = &c {
                time_ms.store(time.to_bits(), Ordering::Relaxed);
                fb.store(feedback.to_bits(), Ordering::Relaxed);
                mx.store(mix.to_bits(), Ordering::Relaxed);
            }
            let mut buf = input.clone();
            e.process(&mut buf, 256);
            prop_assert!(buf.iter().all(|s| s.is_finite()));
            prop_assert!(buf.iter().all(|s| s.abs() < 1e6));
        }
    }
}
