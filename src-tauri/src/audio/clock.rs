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
            wall_clock: SystemClockTicker::with_catchup(
                pipeline_sample_rate,
                engine_block_frames,
                2,
            ),
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
                // Less than one block of headroom left in the ring -- the
                // worker isn't staying ahead of the device.
                if self.primed && queued < block_frames {
                    health::bump(&health::CLOCK_LATE_BLOCKS, 1);
                }
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
