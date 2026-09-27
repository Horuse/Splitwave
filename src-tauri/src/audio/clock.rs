//! Pacing source for the DSP worker.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::audio::health;

const LATE_REPORT_THRESHOLD: Duration = Duration::from_millis(2);
#[cfg(target_os = "linux")]
const RT_BUDGET_RESET_SLEEP: Duration = Duration::from_micros(50);

pub trait ClockSource: Send + 'static {
    /// Returns `false` when `stop` is set; `true` on each tick.
    fn wait_for_tick(&mut self, stop: &AtomicBool) -> bool;

    /// Nominal sample rate this clock targets.
    #[allow(dead_code)]
    fn sample_rate(&self) -> u32;

    fn realtime_ready(&self) -> bool {
        true
    }
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
    report_late: bool,
}

impl SystemClockTicker {
    pub fn new(sample_rate: u32, block_frames: usize) -> Self {
        let period =
            Duration::from_nanos((block_frames as u64 * 1_000_000_000) / sample_rate.max(1) as u64);
        Self {
            sample_rate,
            period,
            next_deadline: None,
            catchup_max: Duration::ZERO,
            report_late: true,
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
                if self.report_late && late >= LATE_REPORT_THRESHOLD {
                    health::bump(&health::CLOCK_LATE_BLOCKS, 1);
                    health::raise_max(&health::CLOCK_LATE_MAX_US, late.as_micros() as u64);
                }
                if late <= self.catchup_max {
                    #[cfg(target_os = "linux")]
                    thread::sleep(RT_BUDGET_RESET_SLEEP);
                    d
                } else {
                    #[cfg(target_os = "linux")]
                    thread::sleep(RT_BUDGET_RESET_SLEEP);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_flag_returns_false_immediately() {
        let stop = AtomicBool::new(true);
        let mut t = SystemClockTicker::new(48_000, 1024);
        assert!(!t.wait_for_tick(&stop));
    }

    #[test]
    fn never_ticks_faster_than_the_block_period() {
        let sr = 48_000;
        let block = 480; // 10 ms
        let mut t = SystemClockTicker::new(sr, block);
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        for _ in 0..10 {
            assert!(t.wait_for_tick(&stop));
        }
        let elapsed = started.elapsed();
        // Nine sleeping intervals must not complete early. There is
        // intentionally no upper bound: a preempted CI runner says nothing
        // about the ticker's pacing contract.
        let want = Duration::from_millis(90);
        assert!(
            elapsed >= want - Duration::from_millis(5),
            "paced too quickly: {elapsed:?}, minimum ~{want:?}"
        );
    }

    #[test]
    fn sample_rate_reports_configured_rate() {
        let t = SystemClockTicker::new(44_100, 1024);
        assert_eq!(t.sample_rate(), 44_100);
    }

    #[test]
    fn bounded_lateness_bursts_through_catchup() {
        let sr = 48_000;
        let block = 480; // 10 ms period
        let mut t = SystemClockTicker::with_catchup(sr, block, 8);
        let stop = AtomicBool::new(false);
        let deadline = Instant::now() - Duration::from_millis(25);
        t.next_deadline = Some(deadline);
        assert!(t.wait_for_tick(&stop));
        assert_eq!(t.next_deadline, Some(deadline + t.period));
    }

    #[test]
    fn beyond_catchup_resets_the_deadline() {
        let sr = 48_000;
        let block = 480; // 10 ms; catchup 8 blocks = 80 ms
        let mut t = SystemClockTicker::with_catchup(sr, block, 8);
        let stop = AtomicBool::new(false);
        let stale = Instant::now() - Duration::from_millis(120);
        t.next_deadline = Some(stale);
        let before = Instant::now();
        assert!(t.wait_for_tick(&stop));
        let reset = t.next_deadline.expect("deadline reset");
        assert!(reset >= before + t.period);
        assert!(reset > stale + t.period);
    }
}
