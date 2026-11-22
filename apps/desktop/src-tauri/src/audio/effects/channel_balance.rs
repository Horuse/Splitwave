use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::ChannelBalanceData;

use super::util::{db_to_linear, load_f32, Ramp};
use super::{Effect, EffectControl};

pub struct ChannelBalanceEffect {
    left: Arc<AtomicU32>,
    right: Arc<AtomicU32>,
    ramp_left: Ramp,
    ramp_right: Ramp,
}

impl ChannelBalanceEffect {
    pub fn new(d: ChannelBalanceData, sample_rate: u32) -> (Self, EffectControl) {
        let left = Arc::new(AtomicU32::new(db_to_linear(d.left_gain_db).to_bits()));
        let right = Arc::new(AtomicU32::new(db_to_linear(d.right_gain_db).to_bits()));
        let control = EffectControl::ChannelBalance {
            left: left.clone(),
            right: right.clone(),
        };
        (Self::from_state(left, right, sample_rate), control)
    }

    pub fn from_state(left: Arc<AtomicU32>, right: Arc<AtomicU32>, sample_rate: u32) -> Self {
        Self {
            ramp_left: Ramp::new(load_f32(&left), sample_rate),
            ramp_right: Ramp::new(load_f32(&right), sample_rate),
            left,
            right,
        }
    }
}

impl Effect for ChannelBalanceEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        self.ramp_left.set(load_f32(&self.left));
        self.ramp_right.set(load_f32(&self.right));
        let stereo = &mut samples[..frames * 2];
        for frame in stereo.chunks_exact_mut(2) {
            frame[0] *= self.ramp_left.next();
            frame[1] *= self.ramp_right.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_applies_per_channel() {
        let (mut e, _) = ChannelBalanceEffect::new(
            ChannelBalanceData {
                left_gain_db: -6.0,
                right_gain_db: 0.0,
                bypassed: false,
            },
            48_000,
        );
        let mut buf = [1.0, 1.0];
        e.process(&mut buf, 1);
        assert!((buf[0] - 0.501).abs() < 0.01);
        assert!((buf[1] - 1.0).abs() < 1e-6);
    }
}
