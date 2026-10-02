use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::audio::graph::MuteData;

use super::util::Ramp;
use super::{Effect, EffectControl};

pub struct MuteEffect {
    muted: Arc<AtomicBool>,
    ramp: Ramp,
}

fn level(muted: bool) -> f32 {
    if muted {
        0.0
    } else {
        1.0
    }
}

impl MuteEffect {
    pub fn new(d: MuteData, sample_rate: u32) -> (Self, EffectControl) {
        let muted = Arc::new(AtomicBool::new(d.muted));
        let control = EffectControl::Mute {
            muted: muted.clone(),
        };
        (
            Self {
                ramp: Ramp::new(level(d.muted), sample_rate),
                muted,
            },
            control,
        )
    }

    pub fn from_state(muted: Arc<AtomicBool>, sample_rate: u32) -> Self {
        Self {
            ramp: Ramp::new(level(muted.load(Ordering::Relaxed)), sample_rate),
            muted,
        }
    }
}

impl Effect for MuteEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        self.ramp.set(level(self.muted.load(Ordering::Relaxed)));
        if self.ramp.at(1.0) {
            return;
        }
        let stereo = &mut samples[..frames * 2];
        if self.ramp.at(0.0) {
            stereo.fill(0.0);
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
    fn mute_zeros() {
        let (mut e, _) = MuteEffect::new(
            MuteData {
                muted: true,
                bypassed: false,
            },
            48_000,
        );
        let mut buf = [0.5, -0.5, 0.3, -0.3];
        e.process(&mut buf, 2);
        assert_eq!(buf, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn mute_control_unmutes_live() {
        let (mut e, c) = MuteEffect::new(
            MuteData {
                muted: true,
                bypassed: false,
            },
            48_000,
        );
        c.apply_update(&serde_json::json!({ "muted": false }));
        let mut buf = vec![0.5_f32; 2 * 240];
        e.process(&mut buf, 240);
        assert_eq!(buf[478], 0.5);
        assert!(buf[0] < 0.01, "fades in, not a step");
    }
}
