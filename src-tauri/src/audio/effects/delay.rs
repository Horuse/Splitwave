use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::DelayData;

use super::util::load_f32;
use super::{Effect, EffectControl};

pub(crate) const MAX_DELAY_MS: f32 = 2000.0;

/// How quickly the delay follows a new time: the gap closes with this time
/// constant...
const GLIDE_MS: f64 = 50.0;
/// ...but never faster than this many frames of delay per frame played, so a
/// change bends the pitch by at most this share, as a tape delay's motor
/// would, rather than jumping (a click) or outrunning the audio (backwards).
const MAX_GLIDE_RATE: f64 = 0.25;

/// Stereo delay line with feedback and dry/wet mix. Ring sized to 2 s @ build
/// SR; a live `time_ms` change moves the read position sample by sample, read
/// between samples, so it glides instead of stepping.
pub struct DelayEffect {
    time_ms: Arc<AtomicU32>,
    feedback: Arc<AtomicU32>,
    mix: Arc<AtomicU32>,
    sample_rate: u32,
    /// Stereo-interleaved ring; capacity = MAX_DELAY_MS @ sample_rate.
    buf: Box<[f32]>,
    /// Next frame written.
    write: usize,
    /// Delay heard now, in frames, gliding toward the set time.
    current_delay_frames: f64,
    /// Share of the gap to the set time closed per frame.
    glide: f64,
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
        let mut effect = Self {
            time_ms,
            feedback,
            mix,
            sample_rate,
            buf: vec![0.0; cap].into_boxed_slice(),
            write: 0,
            current_delay_frames: 0.0,
            glide: 1.0 - (-1000.0 / (GLIDE_MS * sample_rate as f64)).exp(),
        };
        effect.current_delay_frames = effect.target_delay_frames();
        effect
    }

    /// The set time in frames, within what the ring can read between.
    fn target_delay_frames(&self) -> f64 {
        let cap_frames = (self.buf.len() / 2) as f64;
        (load_f32(&self.time_ms).max(1.0) as f64 * 0.001 * self.sample_rate as f64)
            .clamp(1.0, (cap_frames - 2.0).max(1.0))
    }
}

impl Effect for DelayEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        let target = self.target_delay_frames();
        let feedback = load_f32(&self.feedback).clamp(0.0, 0.95);
        let mix = load_f32(&self.mix).clamp(0.0, 1.0);
        let dry = 1.0 - mix;

        let cap = self.buf.len() / 2;
        let stereo = &mut samples[..frames * 2];
        for frame in stereo.chunks_exact_mut(2) {
            let gap = target - self.current_delay_frames;
            if gap != 0.0 {
                let step = (gap * self.glide).clamp(-MAX_GLIDE_RATE, MAX_GLIDE_RATE);
                self.current_delay_frames = if gap.abs() <= step.abs() || gap.abs() < 1e-6 {
                    target
                } else {
                    self.current_delay_frames + step
                };
            }
            let mut pos = self.write as f64 - self.current_delay_frames;
            if pos < 0.0 {
                pos += cap as f64;
            }
            let i0 = pos as usize % cap;
            let i1 = (i0 + 1) % cap;
            let frac = (pos - pos.floor()) as f32;
            let dl = self.buf[i0 * 2] * (1.0 - frac) + self.buf[i1 * 2] * frac;
            let dr = self.buf[i0 * 2 + 1] * (1.0 - frac) + self.buf[i1 * 2 + 1] * frac;
            let il = frame[0];
            let ir = frame[1];
            self.buf[self.write * 2] = il + dl * feedback;
            self.buf[self.write * 2 + 1] = ir + dr * feedback;
            self.write = (self.write + 1) % cap;
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
    fn a_time_change_glides_without_a_step_at_any_block() {
        let rate = 48_000;
        for block in [32, 2048] {
            let (mut e, c) = DelayEffect::new(
                DelayData {
                    time_ms: 100.0,
                    feedback: 0.0,
                    mix: 1.0,
                    bypassed: false,
                },
                rate,
            );
            let EffectControl::Delay { time_ms, .. } = &c else {
                panic!("wrong control variant");
            };
            let tone =
                |n: usize| 0.5 * (std::f32::consts::TAU * 440.0 * n as f32 / rate as f32).sin();
            let mut out = Vec::new();
            let mut n = 0;
            for b in 0..(rate as usize * 2) / block {
                if b * block >= rate as usize / 2 {
                    time_ms.store(400.0f32.to_bits(), Ordering::Relaxed);
                }
                let mut buf: Vec<f32> = (0..block).flat_map(|f| [tone(n + f); 2]).collect();
                n += block;
                e.process(&mut buf, block);
                out.extend(buf.iter().step_by(2).copied());
            }
            // Past the first echo: the tone's own slope, bent by at most the
            // glide's pitch change, and nothing like a jump.
            let worst = out[rate as usize / 5..]
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max);
            let slope = 0.5 * std::f32::consts::TAU * 440.0 / rate as f32;
            assert!(worst < slope * 1.3, "{block}: step of {worst}");
        }
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
