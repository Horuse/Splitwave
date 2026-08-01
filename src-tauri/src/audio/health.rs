//! Process-wide audio glitch counters.
//!
//! RT threads only do `fetch_add(Relaxed)` here; a non-RT tick thread reads
//! the deltas once a second and logs them (see `pipeline::meter`). Counters
//! are global rather than per-stream so a callback can bump one without
//! carrying a handle through every ring helper.

use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! counters {
    ($($(#[$m:meta])* $name:ident),* $(,)?) => {
        $($(#[$m])* pub static $name: AtomicU64 = AtomicU64::new(0);)*

        /// Snapshot in declaration order, paired with a log-friendly label.
        pub fn snapshot() -> Vec<(&'static str, u64)> {
            vec![$((stringify!($name), $name.load(Ordering::Relaxed))),*]
        }
    };
}

counters! {
    /// Samples zero-filled into a device output because the ring ran dry.
    OUTPUT_UNDERRUN_SAMPLES,
    /// Samples dropped pushing into a full ring (consumer fell behind).
    RING_OVERRUN_SAMPLES,
    /// Worker blocks whose clock deadline had already passed on wake.
    CLOCK_LATE_BLOCKS,
    /// Worst single deadline miss, microseconds (monotonic high-water mark).
    CLOCK_LATE_MAX_US,
    /// Availability-paced blocks that gave up waiting and zero-filled.
    AVAILABILITY_TIMEOUTS,
    /// Fatal cpal stream errors reported via the error callback.
    STREAM_ERRORS,
}

/// Name of the high-water-mark counter, whose delta is meaningless.
pub const CLOCK_LATE_MAX_US_NAME: &str = "CLOCK_LATE_MAX_US";

#[inline]
pub fn bump(counter: &AtomicU64, by: u64) {
    if by > 0 {
        counter.fetch_add(by, Ordering::Relaxed);
    }
}

/// Raise a high-water mark without ever lowering it.
#[inline]
pub fn raise_max(counter: &AtomicU64, value: u64) {
    let mut cur = counter.load(Ordering::Relaxed);
    while value > cur {
        match counter.compare_exchange_weak(cur, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(actual) => cur = actual,
        }
    }
}
