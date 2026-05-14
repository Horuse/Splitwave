use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::audio::graph::MuteData;

use super::{Effect, EffectControl};

pub struct MuteEffect {
    muted: Arc<AtomicBool>,
    current: f32,
}

impl MuteEffect {
    pub fn new(d: MuteData) -> (Self, EffectControl) {
        let muted = Arc::new(AtomicBool::new(d.muted));
        let control = EffectControl::Mute {
            muted: muted.clone(),
        };
        (
            Self {
                current: if d.muted { 0.0 } else { 1.0 },
                muted,
            },
            control,
        )
    }

    pub fn from_state(muted: Arc<AtomicBool>) -> Self {
        Self {
            current: if muted.load(Ordering::Relaxed) { 0.0 } else { 1.0 },
            muted,
        }
    }
}

impl Effect for MuteEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let target = if self.muted.load(Ordering::Relaxed) { 0.0 } else { 1.0 };
        if self.current >= 1.0 && target >= 1.0 {
            return;
        }
        let stereo = &mut samples[..frames * 2];
        if self.current <= 0.0 && target <= 0.0 {
            for s in stereo {
                *s = 0.0;
            }
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
    fn mute_zeros() {
        let (mut e, _) = MuteEffect::new(MuteData { muted: true, bypassed: false });
        let mut buf = [0.5, -0.5, 0.3, -0.3];
        e.process(&mut buf, 2);
        assert_eq!(buf, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn mute_control_unmutes_live() {
        let (mut e, c) = MuteEffect::new(MuteData { muted: true, bypassed: false });
        c.apply_update(&serde_json::json!({ "muted": false }));
        let mut buf = [0.5_f32, -0.5];
        e.process(&mut buf, 1);
        assert_eq!(buf, [0.5, -0.5]);
    }
}
