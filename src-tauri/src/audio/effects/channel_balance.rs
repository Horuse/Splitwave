use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::ChannelBalanceData;

use super::util::{db_to_linear, load_f32};
use super::{Effect, EffectControl};

pub struct ChannelBalanceEffect {
    left: Arc<AtomicU32>,
    right: Arc<AtomicU32>,
}

impl ChannelBalanceEffect {
    pub fn new(d: ChannelBalanceData) -> (Self, EffectControl) {
        let left = Arc::new(AtomicU32::new(db_to_linear(d.left_gain_db).to_bits()));
        let right = Arc::new(AtomicU32::new(db_to_linear(d.right_gain_db).to_bits()));
        let control = EffectControl::ChannelBalance {
            left: left.clone(),
            right: right.clone(),
        };
        (Self { left, right }, control)
    }

    pub fn from_state(left: Arc<AtomicU32>, right: Arc<AtomicU32>) -> Self {
        Self { left, right }
    }
}

impl Effect for ChannelBalanceEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let gl = load_f32(&self.left);
        let gr = load_f32(&self.right);
        let stereo = &mut samples[..frames * 2];
        for frame in stereo.chunks_exact_mut(2) {
            frame[0] *= gl;
            frame[1] *= gr;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_applies_per_channel() {
        let (mut e, _) = ChannelBalanceEffect::new(ChannelBalanceData {
            left_gain_db: -6.0,
            right_gain_db: 0.0,
            bypassed: false,
        });
        let mut buf = [1.0, 1.0];
        e.process(&mut buf, 1);
        assert!((buf[0] - 0.501).abs() < 0.01);
        assert!((buf[1] - 1.0).abs() < 1e-6);
    }
}
