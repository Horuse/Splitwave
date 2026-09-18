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
    /// Samples dropped by the capture broadcast into a per-output input ring
    /// (input_bridge.rs) -- a DSP source fell behind the capture callback.
    CAPTURE_RING_OVERRUN_SAMPLES,
    /// Samples dropped pushing into a network receiver ring (stream_recv.rs).
    NET_RING_OVERRUN_SAMPLES,
    /// Samples dropped by a DAG fan-out tap or wire-sender push (dag.rs) --
    /// another output or a wire consumer fell behind this node's block rate.
    TAP_RING_OVERRUN_SAMPLES,
    /// Samples dropped pushing into the speaker worker's output ring
    /// (pipeline/output/mod.rs) -- the cpal callback fell behind the worker.
    SPEAKER_RING_OVERRUN_SAMPLES,
    /// Input samples discarded by a source's backlog trim (SourceState::fill_block).
    SOURCE_TRIM_DROPPED_SAMPLES,
    /// Samples dropped by a StagingRing overrun (producer outran the drain).
    STAGING_OVERRUN_SAMPLES,
    /// Worker blocks whose wall-clock deadline had already passed on wake.
    CLOCK_LATE_BLOCKS,
    /// Worst single deadline miss, microseconds (monotonic high-water mark).
    CLOCK_LATE_MAX_US,
    /// Fatal cpal stream errors reported via the error callback.
    STREAM_ERRORS,
    /// Samples zero-filled on the RT thread because an offloaded effect's
    /// worker had not returned the block in time.
    OFFLOAD_STARVED_SAMPLES,
    /// Samples discarded to restore an offload return ring to its declared
    /// pad after a starve left it permanently deeper.
    OFFLOAD_RESYNC_DROPPED_SAMPLES,
    /// Samples dropped pushing into an offload ring (either direction).
    OFFLOAD_RING_OVERRUN_SAMPLES,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn snapshot_covers_every_counter_in_order() {
        let snap = snapshot();
        assert!(snap.len() >= 12, "all glitch counters snapshot");
        assert_eq!(snap[0].0, "OUTPUT_UNDERRUN_SAMPLES");
        assert_eq!(snap.last().unwrap().0, "OFFLOAD_RING_OVERRUN_SAMPLES");
        // Every name is unique (labels feed tracing logs).
        let mut names: Vec<&str> = snap.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), snap.len(), "counter labels must be unique");
    }

    #[test]
    fn bump_ignores_zero_adds() {
        let before = OUTPUT_UNDERRUN_SAMPLES.load(Ordering::Relaxed);
        bump(&OUTPUT_UNDERRUN_SAMPLES, 0);
        assert_eq!(OUTPUT_UNDERRUN_SAMPLES.load(Ordering::Relaxed), before);
        bump(&OUTPUT_UNDERRUN_SAMPLES, 5);
        assert_eq!(OUTPUT_UNDERRUN_SAMPLES.load(Ordering::Relaxed), before + 5);
    }

    #[test]
    fn raise_max_never_goes_backwards() {
        let before = CLOCK_LATE_MAX_US.load(Ordering::Relaxed);
        raise_max(&CLOCK_LATE_MAX_US, 5_000);
        assert_eq!(CLOCK_LATE_MAX_US.load(Ordering::Relaxed), 5_000);
        // A smaller miss must not lower the high-water mark.
        raise_max(&CLOCK_LATE_MAX_US, 2_000);
        assert_eq!(CLOCK_LATE_MAX_US.load(Ordering::Relaxed), 5_000);
        raise_max(&CLOCK_LATE_MAX_US, 7_000);
        assert_eq!(CLOCK_LATE_MAX_US.load(Ordering::Relaxed), 7_000);
        // Restore the previous value for other tests.
        raise_max(&CLOCK_LATE_MAX_US, before);
    }

    #[test]
    fn raise_max_survives_concurrent_bumps() {
        // Races on the high-water mark resolve to the true maximum, not to
        // whichever CAS got there first.
        let counter = Arc::new(AtomicU64::new(0));
        let handles: Vec<_> = (0..8)
            .map(|k| {
                let c = counter.clone();
                thread::spawn(move || {
                    for i in 0..100u64 {
                        raise_max(&c, (k * 100 + i) as u64);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(counter.load(Ordering::Relaxed), 799);
    }
}
