use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

#[inline]
pub(super) fn db_to_linear(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

#[inline]
pub(super) fn store_f32(slot: &AtomicU32, v: f32) {
    slot.store(v.to_bits(), Ordering::Relaxed);
}

#[inline]
pub(super) fn load_f32(slot: &AtomicU32) -> f32 {
    f32::from_bits(slot.load(Ordering::Relaxed))
}

/// Gain changes glide over this long, whatever the block size: short enough
/// to feel instant, long enough not to click.
pub(super) const GAIN_RAMP_MS: f32 = 5.0;

/// A gain that moves to each new target in a straight line over a fixed
/// number of frames, carried across blocks.
pub(super) struct Ramp {
    current: f32,
    target: f32,
    step: f32,
    remaining: usize,
    frames: usize,
}

impl Ramp {
    pub(super) fn new(value: f32, sample_rate: u32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
            frames: ((GAIN_RAMP_MS * sample_rate as f32 / 1000.0) as usize).max(1),
        }
    }

    #[inline]
    pub(super) fn set(&mut self, target: f32) {
        if target != self.target {
            self.target = target;
            self.remaining = self.frames;
            self.step = (target - self.current) / self.frames as f32;
        }
    }

    /// Settled at `value`: no glide in progress.
    #[inline]
    pub(super) fn at(&self, value: f32) -> bool {
        self.remaining == 0 && self.current == value
    }

    #[inline]
    pub(super) fn is_settled(&self) -> bool {
        self.remaining == 0
    }

    #[inline]
    pub(super) fn value(&self) -> f32 {
        self.current
    }

    #[inline]
    pub(super) fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.current = if self.remaining == 0 {
                self.target
            } else {
                self.current + self.step
            };
        }
        self.current
    }
}

pub(super) fn num(data: &Value, key: &str) -> Option<f32> {
    data.get(key).and_then(Value::as_f64).map(|v| v as f32)
}
