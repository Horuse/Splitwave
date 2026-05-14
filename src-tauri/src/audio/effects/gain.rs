use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::GainData;

use super::util::{db_to_linear, load_f32};
use super::{Effect, EffectControl};

pub struct GainEffect {
    linear: Arc<AtomicU32>,
    current: f32,
}

impl GainEffect {
    pub fn new(d: GainData) -> (Self, EffectControl) {
        let initial = db_to_linear(d.gain_db);
        let linear = Arc::new(AtomicU32::new(initial.to_bits()));
        let control = EffectControl::Gain {
            linear: linear.clone(),
        };
        (
            Self {
                linear,
                current: initial,
            },
            control,
        )
    }

    pub fn from_state(linear: Arc<AtomicU32>) -> Self {
        Self {
            current: load_f32(&linear),
            linear,
        }
    }
}

impl Effect for GainEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let target = load_f32(&self.linear);
        let stereo = &mut samples[..frames * 2];
        if (self.current - target).abs() < 1e-7 {
            for s in stereo {
                *s *= target;
            }
            self.current = target;
            return;
        }
        let step = (target - self.current) / frames as f32;
        let mut g = self.current;
        for frame in stereo.chunks_exact_mut(2) {
            g += step;
            frame[0] *= g;
            frame[1] *= g;
        }
        self.current = target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_applies_db() {
        let (mut e, _) = GainEffect::new(GainData { gain_db: 6.0, bypassed: false });
        let mut buf = [1.0_f32, 1.0];
        e.process(&mut buf, 1);
        assert!((buf[0] - 1.995).abs() < 0.01);
    }

    #[test]
    fn gain_control_changes_live() {
        let (mut e, c) = GainEffect::new(GainData { gain_db: 0.0, bypassed: false });
        c.apply_update(&serde_json::json!({ "gainDb": 6.0 }));
        let mut buf = [1.0_f32, 1.0];
        e.process(&mut buf, 1);
        assert!((buf[0] - 1.995).abs() < 0.01);
    }
}
