//! Pacing source for the DSP worker.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::audio::health;

const LATE_REPORT_THRESHOLD: Duration = Duration::from_millis(2);

pub trait ClockSource: Send + 'static {
    /// Returns `false` when `stop` is set; `true` on each tick.
    fn wait_for_tick(&mut self, stop: &AtomicBool) -> bool;

    /// Nominal sample rate this clock targets.
    #[allow(dead_code)]
    fn sample_rate(&self) -> u32;
}

/// On overrun, the next deadline resets to "now" rather than bursting through
/// accumulated ticks (which would put rings straight back into desync).
///
/// With `catchup`, a bounded overrun keeps the old deadline so the missed
/// ticks fire back-to-back. Needed when downstream is elastic (network send
/// rings): losing the time means the capture ring outgrows its backlog cap
/// and gets spliced, baking a click into the wire.
pub struct SystemClockTicker {
    #[allow(dead_code)]
    sample_rate: u32,
    period: Duration,
    next_deadline: Option<Instant>,
    catchup_max: Duration,
}

impl SystemClockTicker {
    pub fn new(sample_rate: u32, block_frames: usize) -> Self {
        let period = Duration::from_nanos(
            (block_frames as u64 * 1_000_000_000) / sample_rate.max(1) as u64,
        );
        Self {
            sample_rate,
            period,
            next_deadline: None,
            catchup_max: Duration::ZERO,
        }
    }

    /// Burst through up to `max_blocks` of accumulated lag; beyond that the
    /// deadline resets (a real stall, not a scheduler hiccup).
    pub fn with_catchup(sample_rate: u32, block_frames: usize, max_blocks: u32) -> Self {
        let mut t = Self::new(sample_rate, block_frames);
        t.catchup_max = t.period * max_blocks;
        t
    }
}

impl ClockSource for SystemClockTicker {
    fn wait_for_tick(&mut self, stop: &AtomicBool) -> bool {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        let now = Instant::now();
        let anchor = match self.next_deadline {
            Some(d) if d > now => {
                thread::sleep(d - now);
                d
            }
            Some(d) => {
                let late = now - d;
                // Sub-threshold lateness is scheduler jitter the next deadline
                // absorbs; only a real block-scale miss is worth reporting.
                if late >= LATE_REPORT_THRESHOLD {
                    health::bump(&health::CLOCK_LATE_BLOCKS, 1);
                    health::raise_max(&health::CLOCK_LATE_MAX_US, late.as_micros() as u64);
                }
                if late <= self.catchup_max {
                    d
                } else {
                    now
                }
            }
            None => now,
        };
        self.next_deadline = Some(anchor + self.period);
        !stop.load(Ordering::SeqCst)
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}
