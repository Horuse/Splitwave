//! How much captured audio a live source keeps queued ahead of the graph.
//!
//! A capture device hands audio over in bursts on its own clock while the
//! graph takes one block per output callback, so the queue saws up and down.
//! `DepthEstimator` sizes the headroom from that saw; `Cushion` holds the
//! queue's mean there by splicing a few frames out (capture clock faster, or
//! startup backlog) or stretching a few in (capture clock slower), sparsely
//! and with crossfades, so the correction is inaudible and never adds latency
//! of its own.

use crate::audio::adaptive_depth::DepthEstimator;

/// Correction the source should apply before reading its next block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Adjust {
    None,
    /// Splice this many frames out of the queue.
    Drop(usize),
    /// Stretch the queue by repeating this many frames.
    Insert(usize),
}

/// Largest single splice. Bigger corrections are spread over several.
pub(super) const MAX_SPLICE_FRAMES: usize = 64;

/// One dip measurement spans this long; it has to cover the slowest capture
/// burst (Bluetooth and ScreenCaptureKit deliver every ~20 ms).
const WINDOW_MS: f64 = 250.0;
/// Headroom assumed before anything is measured: a typical 512-frame capture
/// buffer at 48 kHz.
const PRIOR_MS: f64 = 10.0;
/// Kept above the measured dip at all times.
const SAFETY_MS: f64 = 1.0;
/// One splice per this much audio at most, so time is never compressed or
/// stretched by more than ~6% while a correction runs.
const SPLICE_EVERY_MS: f64 = 21.0;

pub(super) struct Cushion {
    need: usize,
    safety: usize,
    depth: DepthEstimator,
    sum: f64,
    blocks: usize,
    low: usize,
    /// Correction still owed: negative drops, positive inserts.
    owed: i64,
    cooldown: usize,
    cooldown_blocks: usize,
    primed: bool,
    /// Frames missed since the queue last ran dry, while it refills.
    outage: Option<usize>,
}

impl Cushion {
    /// `need` is the frames the graph takes per block, in the queue's rate.
    pub(super) fn new(need: usize, sample_rate: u32) -> Self {
        let frames = |ms: f64| ((sample_rate as f64 * ms / 1000.0).round() as usize).max(1);
        Self {
            need,
            safety: frames(SAFETY_MS).max(16),
            depth: DepthEstimator::new(sample_rate, need, WINDOW_MS, frames(PRIOR_MS)),
            sum: 0.0,
            blocks: 0,
            low: usize::MAX,
            owed: 0,
            cooldown: 0,
            cooldown_blocks: (frames(SPLICE_EVERY_MS) / need.max(1)).max(1),
            primed: false,
            outage: None,
        }
    }

    /// Lowest the queue may sit just before a read. One block is the read;
    /// a second covers phase: the queue is only sampled at reads, so as the
    /// two clocks slide past each other its sampled low jumps by up to a
    /// block from one capture burst to the next.
    fn floor(&self) -> usize {
        2 * self.need + self.safety
    }

    /// Mean queue depth to hold: the floor plus the measured dip.
    pub(super) fn target(&self) -> usize {
        self.floor() + self.depth.depth()
    }

    /// Before the source's first read, and again after it ran dry. `None`
    /// while the queue is still filling: play silence for this block. Once it
    /// reaches the target, everything beyond it is backlog nobody has heard,
    /// so it can go at once: returns the frames to discard.
    ///
    /// A refill after running dry is one outage, however many blocks it
    /// spans; it is charged to the depth once, sized by everything missed.
    pub(super) fn prime(&mut self, queued: usize) -> Option<usize> {
        if queued < self.target() {
            if let Some(missed) = self.outage.as_mut() {
                *missed += self.need;
            }
            return None;
        }
        if let Some(missed) = self.outage.take() {
            self.depth.underrun(missed);
        }
        self.primed = true;
        self.owed = 0;
        self.sum = 0.0;
        self.blocks = 0;
        self.low = usize::MAX;
        Some(queued.saturating_sub(self.target()))
    }

    pub(super) fn is_primed(&self) -> bool {
        self.primed
    }

    /// The graph ran `missing` frames dry mid-block: refill before playing
    /// on, rather than limp along a starved queue one click per block.
    pub(super) fn underrun(&mut self, missing: usize) {
        self.primed = false;
        self.outage = Some(self.outage.unwrap_or(0) + missing);
    }

    /// Asymmetric on purpose. Running short is an audible dropout, so any
    /// window whose emptiest point ate into half the safety is topped up.
    /// Running long only costs latency, so the mean may overshoot by a quarter
    /// of the headroom before frames are dropped; the saw never triggers it.
    fn correction(&self, mean: f64, low: usize) -> i64 {
        let floor = self.floor();
        if low + self.safety / 2 < floor {
            return (floor - low) as i64;
        }
        let over = mean - self.target() as f64;
        let slack = (self.depth.depth() / 4).max(self.safety) as f64;
        if over > slack {
            // Never cut below the floor at the window's emptiest point.
            return -(over.min((low - floor) as f64).round() as i64);
        }
        0
    }

    /// Call once per block with the frames queued before the read. Returns the
    /// splice to perform now, if one is due and the queue can afford it.
    pub(super) fn observe(&mut self, queued: usize) -> Adjust {
        self.sum += queued as f64;
        self.blocks += 1;
        self.low = self.low.min(queued);
        if self.depth.observe(queued) {
            let mean = self.sum / self.blocks as f64;
            let low = self.low;
            self.sum = 0.0;
            self.blocks = 0;
            self.low = usize::MAX;
            if self.owed == 0 {
                self.owed = self.correction(mean, low);
            }
        }
        if self.cooldown > 0 {
            self.cooldown -= 1;
            return Adjust::None;
        }
        let fade = super::dag::SPLICE_FADE_FRAMES;
        if self.owed < 0 {
            let n = ((-self.owed) as usize).min(MAX_SPLICE_FRAMES);
            // A drop reads a fade on each side of the cut and must leave the
            // block itself behind.
            if queued >= n + 2 * fade + self.need {
                self.owed += n as i64;
                self.cooldown = self.cooldown_blocks;
                return Adjust::Drop(n);
            }
        } else if self.owed > 0 {
            let n = (self.owed as usize).min(MAX_SPLICE_FRAMES);
            if queued >= n + fade {
                self.owed -= n as i64;
                self.cooldown = self.cooldown_blocks;
                return Adjust::Insert(n);
            }
        }
        Adjust::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    struct Run {
        late_underruns: usize,
        mean_queued: f64,
        splices: usize,
        max_queued: usize,
    }

    /// Event-driven model of a capture device feeding the queue in bursts on
    /// its own clock while an output device drains one block at a time.
    /// `jitter` delays each capture burst by a pseudo-random share of it.
    fn simulate(
        block: usize,
        burst: usize,
        capture_ppm: f64,
        jitter: f64,
        initial_backlog: usize,
        seconds: f64,
        settle_s: f64,
    ) -> Run {
        let mut c = Cushion::new(block, SR);
        let out_period = block as f64 / SR as f64;
        let in_period = burst as f64 / (SR as f64 * (1.0 + capture_ppm * 1e-6));
        let mut queued = initial_backlog;
        let mut next_in = in_period * 0.37;
        let mut late = 0.0;
        let mut t_out = 0.0;
        let mut seed: u32 = 0x2545_f491;
        let mut run = Run {
            late_underruns: 0,
            mean_queued: 0.0,
            splices: 0,
            max_queued: 0,
        };
        let mut samples = 0usize;
        while t_out < seconds {
            if next_in + late <= t_out {
                queued += burst;
                next_in += in_period;
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                late = jitter * in_period * (seed % 1000) as f64 / 1000.0;
                continue;
            }
            if !c.is_primed() {
                match c.prime(queued) {
                    Some(excess) => queued -= excess,
                    None => {
                        t_out += out_period;
                        continue;
                    }
                }
            }
            match c.observe(queued) {
                Adjust::Drop(n) => {
                    queued -= n;
                    run.splices += 1;
                }
                Adjust::Insert(n) => {
                    queued += n;
                    run.splices += 1;
                }
                Adjust::None => {}
            }
            if queued < block {
                if t_out > settle_s {
                    run.late_underruns += 1;
                }
                c.underrun(block - queued);
                queued = 0;
            } else {
                queued -= block;
            }
            if t_out > settle_s {
                run.mean_queued += queued as f64;
                run.max_queued = run.max_queued.max(queued);
                samples += 1;
            }
            t_out += out_period;
        }
        run.mean_queued /= samples.max(1) as f64;
        run
    }

    #[test]
    fn startup_backlog_is_dropped_before_anything_plays() {
        let mut c = Cushion::new(64, SR);
        assert_eq!(c.prime(10), None, "still filling");
        assert!(!c.is_primed());
        let excess = c.prime(20_000).expect("primed");
        assert_eq!(20_000 - excess, c.target());
        assert!(c.is_primed());
    }

    #[test]
    fn same_clock_settles_near_one_capture_burst() {
        for (block, burst) in [(32, 512), (64, 512), (128, 256), (256, 480), (1024, 512)] {
            let r = simulate(block, burst, 0.0, 0.0, 4_800, 30.0, 10.0);
            assert_eq!(
                r.late_underruns, 0,
                "{block}/{burst}: underruns once settled"
            );
            let bound = (burst + 2 * block + 2 * 48) as f64;
            assert!(
                r.mean_queued < bound,
                "{block}/{burst}: mean queue {} frames, bound {bound}",
                r.mean_queued
            );
        }
    }

    #[test]
    fn drifting_capture_clock_never_starves_or_accumulates() {
        // 200 ppm is far beyond real crystal error; 100 s drifts ~1000 frames.
        for block in [32, 64, 256, 1024] {
            for burst in [256, 512, 960] {
                for ppm in [-200.0, -50.0, 50.0, 200.0] {
                    let r = simulate(block, burst, ppm, 0.0, 0, 100.0, 10.0);
                    let case = format!("{block}/{burst} @ {ppm} ppm");
                    assert_eq!(r.late_underruns, 0, "{case}: underran");
                    let bound = burst + 3 * block + 1_024;
                    assert!(r.max_queued < bound, "{case}: max queue {}", r.max_queued);
                    assert!(r.splices < 200, "{case}: {} splices", r.splices);
                }
            }
        }
    }

    #[test]
    fn jittery_capture_buys_headroom_instead_of_glitching() {
        // A loaded VM: every burst lands up to 80% of its period late.
        let r = simulate(64, 480, 0.0, 0.8, 0, 120.0, 30.0);
        assert_eq!(r.late_underruns, 0);
        assert!(r.mean_queued < 3.0 * 480.0, "mean queue {}", r.mean_queued);
    }

    #[test]
    fn corrections_stay_sparse() {
        let r = simulate(64, 512, 0.0, 0.0, 0, 60.0, 10.0);
        assert!(
            r.splices < 10,
            "{} splices under matching clocks",
            r.splices
        );
    }

    #[test]
    fn a_run_of_empty_blocks_is_one_outage() {
        let mut c = Cushion::new(32, SR);
        c.prime(c.target()).expect("primed");
        let before = c.target();
        c.underrun(32);
        // 20 ms of silence while the capture refills.
        for _ in 0..30 {
            assert_eq!(c.prime(0), None);
        }
        let excess = c.prime(100_000).expect("refilled");
        let grown = c.target() - before;
        let missed = 32 + 30 * 32;
        assert!(grown >= missed, "depth must cover the outage: +{grown}");
        assert!(grown < 2 * missed, "one outage, not thirty: +{grown}");
        assert_eq!(100_000 - excess, c.target());
    }

    #[test]
    fn a_splice_waits_until_the_queue_can_afford_it() {
        let mut c = Cushion::new(64, SR);
        c.owed = -64;
        assert_eq!(c.observe(64), Adjust::None, "not enough queued to cut");
        assert_eq!(c.observe(64 + 64 + 2 * 32), Adjust::Drop(64));
    }
}
