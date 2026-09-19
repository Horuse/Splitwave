//! Pacing source for the DSP worker.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
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

    fn rate_limiter(sample_rate: u32, block_frames: usize) -> Self {
        let mut ticker = Self::new(sample_rate, block_frames);
        ticker.report_late = false;
        ticker
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

/// Cap on the sleep between ring-level checks, so `stop` stays responsive
/// even when the ring is far above target.
const FILL_CLOCK_MAX_SLEEP: Duration = Duration::from_millis(5);

/// Paces the speaker DSP worker off the speaker ring's own fill level rather
/// than a wall-clock deadline. A late block just means the ring is below
/// target, so the worker produces the next block immediately and never loses
/// the notion of "how far behind" the way a deadline reset would.
pub struct DeviceFillClock {
    pipeline_sample_rate: u32,
    device_sample_rate: Arc<AtomicU32>,
    engine_block_frames: usize,
    level: Arc<AtomicI64>,
    /// Fill target, sized to the device's own buffer by the audio callback
    /// (see `speaker_ring`). Read here every tick so the ring always bridges
    /// one full callback whatever buffer the device negotiated.
    target: Arc<AtomicI64>,
    /// The startup fill budget has been produced. Until then an empty ring is
    /// startup prefill, not a worker that fell behind.
    primed: bool,
    startup_frames: usize,
    /// Prevents a sink that drains immediately (for example a PipeWire null
    /// sink) from turning the real-time worker into an unbounded busy loop.
    wall_clock: SystemClockTicker,
}

impl DeviceFillClock {
    pub fn new(
        pipeline_sample_rate: u32,
        device_sample_rate: Arc<AtomicU32>,
        engine_block_frames: usize,
        level: Arc<AtomicI64>,
        target: Arc<AtomicI64>,
    ) -> Self {
        Self {
            pipeline_sample_rate,
            device_sample_rate,
            engine_block_frames,
            level,
            target,
            primed: false,
            startup_frames: 0,
            wall_clock: SystemClockTicker::rate_limiter(pipeline_sample_rate, engine_block_frames),
        }
    }
}

impl ClockSource for DeviceFillClock {
    fn wait_for_tick(&mut self, stop: &AtomicBool) -> bool {
        loop {
            if stop.load(Ordering::SeqCst) {
                return false;
            }
            let target_frames = self.target.load(Ordering::Relaxed).max(0) as usize;
            let queued = self.level.load(Ordering::Relaxed).max(0) as usize;
            let dev_sr = self.device_sample_rate.load(Ordering::Relaxed).max(1);
            let pipe_sr = self.pipeline_sample_rate.max(1) as u64;
            let block_frames = ((self.engine_block_frames as u64 * dev_sr as u64 + pipe_sr / 2)
                / pipe_sr) as usize;
            if !self.primed {
                self.startup_frames = self.startup_frames.saturating_add(block_frames);
                self.primed = self.startup_frames >= target_frames;
                return true;
            }
            if queued + block_frames <= target_frames {
                return self.wall_clock.wait_for_tick(stop);
            }
            let overshoot = queued + block_frames - target_frames;
            let drain = Duration::from_nanos((overshoot as u64 * 1_000_000_000) / dev_sr as u64);
            thread::sleep(drain.min(FILL_CLOCK_MAX_SLEEP));
        }
    }

    fn sample_rate(&self) -> u32 {
        self.device_sample_rate.load(Ordering::Relaxed)
    }

    fn realtime_ready(&self) -> bool {
        self.primed
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
    fn paces_at_the_block_period() {
        let sr = 48_000;
        let block = 480; // 10 ms
        let mut t = SystemClockTicker::new(sr, block);
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        for _ in 0..10 {
            assert!(t.wait_for_tick(&stop));
        }
        let elapsed = started.elapsed();
        // 9 sleeping intervals ≈ 9 periods. Under parallel test load the
        // scheduler can overshoot, so only a coarse band is asserted:
        // no faster than pacing allows, no wildly slower.
        let want = Duration::from_millis(90);
        assert!(
            elapsed >= want - Duration::from_millis(5)
                && elapsed < want + Duration::from_millis(200),
            "paced {elapsed:?}, want ~{want:?}"
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

    #[test]
    fn rate_limiter_does_not_report_late() {
        // The device-fill clock shares the ticker with report_late off.
        let mut t = SystemClockTicker::rate_limiter(48_000, 480);
        let stop = AtomicBool::new(false);
        let before = health::CLOCK_LATE_BLOCKS.load(Ordering::Relaxed);
        t.next_deadline = Some(Instant::now() - Duration::from_millis(25));
        assert!(t.wait_for_tick(&stop));
        assert_eq!(health::CLOCK_LATE_BLOCKS.load(Ordering::Relaxed), before);
    }

    #[test]
    fn fill_clock_primes_until_the_startup_budget_is_met() {
        let dev_sr = Arc::new(AtomicU32::new(48_000));
        let level = Arc::new(AtomicI64::new(0));
        let target = Arc::new(AtomicI64::new(2 * 1024)); // two engine blocks
        let mut clock =
            DeviceFillClock::new(48_000, dev_sr.clone(), 1024, level.clone(), target.clone());
        let stop = AtomicBool::new(false);
        assert!(!clock.realtime_ready(), "fresh clock is unprimed");
        assert!(clock.wait_for_tick(&stop));
        assert!(!clock.realtime_ready(), "one block is not the full budget");
        assert!(clock.wait_for_tick(&stop));
        assert!(clock.realtime_ready(), "budget met after the second block");
        // Primed clock paces off the ring: below target → wall-clock tick.
        assert!(clock.wait_for_tick(&stop));
        // sample_rate follows the device atom.
        assert_eq!(clock.sample_rate(), 48_000);
        dev_sr.store(96_000, Ordering::Relaxed);
        assert_eq!(clock.sample_rate(), 96_000);
    }

    #[test]
    fn fill_clock_waits_when_the_ring_is_over_target() {
        let dev_sr = Arc::new(AtomicU32::new(48_000));
        let level = Arc::new(AtomicI64::new(1024));
        let target = Arc::new(AtomicI64::new(1024)); // already at target
        let mut clock = DeviceFillClock::new(48_000, dev_sr, 1024, level.clone(), target.clone());
        let stop = AtomicBool::new(false);
        // First call primes. Stop the otherwise blocking second call after it
        // has demonstrably waited instead of returning immediately.
        assert!(clock.wait_for_tick(&stop));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_setter = stop.clone();
        let join = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(15));
            stop_setter.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        assert!(!clock.wait_for_tick(&stop));
        join.join().unwrap();
        assert!(start.elapsed() >= Duration::from_millis(10));
    }

    #[test]
    fn fill_clock_honours_stop() {
        let dev_sr = Arc::new(AtomicU32::new(48_000));
        let mut clock = DeviceFillClock::new(
            48_000,
            dev_sr,
            1024,
            Arc::new(AtomicI64::new(0)),
            Arc::new(AtomicI64::new(1024)),
        );
        let stop = AtomicBool::new(true);
        assert!(!clock.wait_for_tick(&stop));
    }
}
