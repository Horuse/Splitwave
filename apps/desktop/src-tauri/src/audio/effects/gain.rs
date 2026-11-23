use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::GainData;

use super::util::{db_to_linear, load_f32, Ramp};
use super::{Effect, EffectControl};

pub struct GainEffect {
    linear: Arc<AtomicU32>,
    ramp: Ramp,
}

impl GainEffect {
    pub fn new(d: GainData, sample_rate: u32) -> (Self, EffectControl) {
        let initial = db_to_linear(d.gain_db);
        let linear = Arc::new(AtomicU32::new(initial.to_bits()));
        let control = EffectControl::Gain {
            linear: linear.clone(),
        };
        (
            Self {
                linear,
                ramp: Ramp::new(initial, sample_rate),
            },
            control,
        )
    }

    pub fn from_state(linear: Arc<AtomicU32>, sample_rate: u32) -> Self {
        Self {
            ramp: Ramp::new(load_f32(&linear), sample_rate),
            linear,
        }
    }
}

impl Effect for GainEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        self.ramp.set(load_f32(&self.linear));
        let stereo = &mut samples[..frames * 2];
        if self.ramp.is_settled() {
            let g = self.ramp.value();
            for s in stereo {
                *s *= g;
            }
            return;
        }
        for frame in stereo.chunks_exact_mut(2) {
            let g = self.ramp.next();
            frame[0] *= g;
            frame[1] *= g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_applies_db() {
        let (mut e, _) = GainEffect::new(
            GainData {
                gain_db: 6.0,
                bypassed: false,
            },
            48_000,
        );
        let mut buf = [1.0_f32, 1.0];
        e.process(&mut buf, 1);
        assert!((buf[0] - 1.995).abs() < 0.01);
    }

    #[test]
    fn gain_control_changes_live() {
        let (mut e, c) = GainEffect::new(
            GainData {
                gain_db: 0.0,
                bypassed: false,
            },
            48_000,
        );
        c.apply_update(&serde_json::json!({ "gainDb": 6.0 }));
        let mut buf = vec![1.0_f32; 2 * 240];
        e.process(&mut buf, 240);
        assert!((buf[478] - 1.995).abs() < 0.01);
    }

    #[test]
    fn a_change_glides_over_the_same_time_at_any_block() {
        for block in [32, 2048] {
            let (mut e, c) = GainEffect::new(
                GainData {
                    gain_db: -60.0,
                    bypassed: false,
                },
                48_000,
            );
            c.apply_update(&serde_json::json!({ "gainDb": 0.0 }));
            let mut out = Vec::new();
            for _ in 0..4096 / block {
                let mut buf = vec![1.0_f32; 2 * block];
                e.process(&mut buf, block);
                out.extend(buf.iter().step_by(2).copied());
            }
            // Halfway through 5 ms (240 frames at 48 kHz).
            assert!((out[119] - 0.5).abs() < 0.01, "{block}: {}", out[119]);
            assert_eq!(out[239], 1.0, "{block}");
        }
    }
}
