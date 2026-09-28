//! How much captured audio a live source keeps queued ahead of the graph.
//!
//! A capture device hands audio over in bursts on its own clock while the
//! graph takes one block per output callback, so the queue saws up and down.
//! `DepthEstimator` sizes the headroom from that saw; `Cushion` holds the
//! queue's mean there by splicing a few frames out (capture clock faster, or
//! startup backlog) or stretching a few in (capture clock slower), sparsely
//! and with crossfades, so the correction is inaudible and never adds latency
//! of its own.
//!
//! Running dry deepens the queue only by the audio that turns out to have
//! been late (`OutageJudge`). A source that goes quiet -- a paused app, a
//! muted device -- leaves the depth where it was.
//!
//! A source on the output's own clock (`Cushion::locked`) has no drift to
//! correct, so it is never spliced: any correction there could only chase
//! delivery jitter, and every one is audible. Its queue is set once, when it
//! starts or restarts from silence, and left alone while it plays.

use crate::audio::adaptive_depth::{DepthEstimator, OutageJudge};

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
/// Kept above the measured dip at all times. A process tap's p99.9 delivery
/// jitter is already ~1.8 ms, so a thinner margin clicks every few seconds.
const SAFETY_MS: f64 = 2.0;
/// One splice per this much audio at most, so time is never compressed or
/// stretched by more than ~6% while a correction runs.
const SPLICE_EVERY_MS: f64 = 21.0;
/// Ceiling on the measured headroom. Jitter beyond this is a broken capture,
/// and a deeper target would outgrow the one-second input ring and never
/// finish priming.
const MAX_DEPTH_MS: f64 = 250.0;
/// On a locked source, a late delivery deepens the queue only if another came
/// within this long. A lone stall costs one dropout; paying for it with
/// latency for the next minute would cost every sound after it.
const LATE_REPEAT_MS: f64 = 60_000.0;

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
    /// Has played before: a re-prime refills a stream in progress.
    started: bool,
    /// Same clock as the output: never splice while playing.
    locked: bool,
    /// A locked queue starts from the prior; the first measurement corrects
    /// it once, the way a startup offset is removed, and never again.
    settled: bool,
    judge: OutageJudge,
    max_depth: usize,
    /// Blocks since the last late delivery was seen, for `LATE_REPEAT_MS`.
    since_late: usize,
    late_repeat_blocks: usize,
    /// Largest recent arrival between two reads: the saw's height.
    quantum: usize,
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
            started: false,
            locked: false,
            settled: false,
            judge: OutageJudge::new(sample_rate),
            max_depth: frames(MAX_DEPTH_MS),
            since_late: usize::MAX,
            late_repeat_blocks: frames(LATE_REPEAT_MS) / need.max(1),
            quantum: 0,
        }
    }

    /// For a source on the output's own clock.
    pub(super) fn locked(need: usize, sample_rate: u32) -> Self {
        Self::new(need, sample_rate).into_locked()
    }

    pub(super) fn into_locked(self) -> Self {
        Self {
            locked: true,
            ..self
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
        self.floor() + self.depth.depth().min(self.max_depth)
    }

    /// Once per block, before anything else: the frames the source delivered
    /// since the last block. A gap's verdict lands here, once the flow after
    /// it shows whether the missing audio was late or never existed.
    pub(super) fn arrived(&mut self, frames: usize) {
        self.since_late = self.since_late.saturating_add(1);
        // The largest single arrival is how far the queue saws between reads;
        // it sinks back slowly if deliveries get smaller. Only steady flow
        // counts: a catch-up burst after a gap is the gap, not the saw.
        if self.primed && self.judge.is_clear() {
            let steady = if self.quantum > 0 {
                frames.min(2 * self.quantum)
            } else {
                frames
            };
            self.quantum = steady.max(self.quantum - self.quantum / 4096);
        }
        if let Some(late) = self.judge.delivered(frames, self.need) {
            if late > 0 {
                if !self.locked || self.since_late <= self.late_repeat_blocks {
                    self.depth.underrun(late);
                }
                self.since_late = 0;
            }
        }
    }

    /// Queue to start playing from. Priming ends right after a delivery, at
    /// the top of the saw, so it must last a whole saw down to the floor, plus
    /// whatever jitter the measured dip shows beyond an ideal saw (whose dip
    /// is half its height). Counting the dip twice would double the jitter
    /// allowance along with the saw.
    ///
    /// Only a locked queue starts this tight: it is never corrected while it
    /// plays, so the level it starts at is the level it keeps. A drifting one
    /// starts a whole dip higher and lets its splices settle it.
    fn start_level(&self) -> usize {
        let dip = self.depth.depth().min(self.max_depth);
        if !self.locked || self.quantum == 0 {
            return self.target() + dip;
        }
        let jitter = dip.saturating_sub(self.quantum / 2);
        self.floor() + self.quantum.max(dip) + jitter
    }

    /// Before the source's first read, and again after it ran dry. `None`
    /// while the queue is still filling: play silence for this block.
    ///
    /// Returns the frames to discard. The first start always has them: what
    /// piled up before anything played is backlog nobody has heard.
    ///
    /// After running dry it depends on the clock. A locked source realigns
    /// here, under the silence it just played: with no drift, anything beyond
    /// the start level is audio that arrived late for time already filled
    /// with silence, and dropping exactly that keeps its latency where it was.
    /// A drifting source keeps what arrived (it is the stream picking up where
    /// it stopped) and lets the ordinary splices trim the excess.
    pub(super) fn prime(&mut self, queued: usize) -> Option<usize> {
        let start = self.start_level();
        if queued < start {
            return None;
        }
        let excess = if self.started && !self.locked {
            0
        } else {
            queued - start
        };
        self.started = true;
        self.primed = true;
        self.owed = 0;
        self.sum = 0.0;
        self.blocks = 0;
        self.low = usize::MAX;
        // Deliveries right after a gap come in catch-up bursts; measuring them
        // would read the gap's aftermath as the source's everyday jitter.
        self.depth.discard_window();
        Some(excess)
    }

    pub(super) fn is_primed(&self) -> bool {
        self.primed
    }

    /// The graph ran `missing` frames dry mid-block: refill before playing
    /// on, rather than limp along a starved queue one click per block.
    pub(super) fn underrun(&mut self, missing: usize) {
        self.primed = false;
        self.judge.missing(missing);
        // The window holds the drain, which says nothing about jitter.
        self.depth.discard_window();
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
        // While a correction runs the queue moves by design, and a window
        // holding that move would read it as jitter.
        if self.owed != 0 {
            self.depth.discard_window();
            self.sum = 0.0;
            self.blocks = 0;
            self.low = usize::MAX;
        } else {
            self.sum += queued as f64;
            self.blocks += 1;
            self.low = self.low.min(queued);
        }
        if self.owed == 0 && self.depth.observe(queued) {
            let mean = self.sum / self.blocks as f64;
            let low = self.low;
            self.sum = 0.0;
            self.blocks = 0;
            self.low = usize::MAX;
            if self.owed == 0 && !self.locked {
                self.owed = self.correction(mean, low);
            } else if self.locked && !self.settled && self.depth.is_measured() {
                self.settled = true;
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
        simulate_with_quiet(
            block,
            burst,
            capture_ppm,
            jitter,
            initial_backlog,
            seconds,
            settle_s,
            &[],
        )
    }

    /// As `simulate`, with spans where the source delivers nothing at all
    /// (its audio does not exist, as with a paused app).
    #[allow(clippy::too_many_arguments)]
    fn simulate_with_quiet(
        block: usize,
        burst: usize,
        capture_ppm: f64,
        jitter: f64,
        initial_backlog: usize,
        seconds: f64,
        settle_s: f64,
        quiet: &[(f64, f64)],
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
        let mut arrived = 0usize;
        while t_out < seconds {
            if next_in + late <= t_out {
                if !quiet.iter().any(|&(a, b)| next_in >= a && next_in < b) {
                    queued += burst;
                    arrived += burst;
                }
                next_in += in_period;
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                late = jitter * in_period * (seed % 1000) as f64 / 1000.0;
                continue;
            }
            c.arrived(std::mem::take(&mut arrived));
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
                let quiet_now = quiet
                    .iter()
                    .any(|&(a, b)| t_out >= a - 0.05 && t_out < b + 0.05);
                if t_out > settle_s && !quiet_now {
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
        assert_eq!(20_000 - excess, c.start_level());
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

    /// A second of ordinary flow (`burst` frames every `burst` frames of
    /// time), then `gap` frames of time with nothing delivered, then
    /// `catch_up` extra frames on the first delivery after it, then ordinary
    /// flow until the verdict is in. The consumer runs dry right as the gap
    /// starts, the worst case.
    fn gap_then(c: &mut Cushion, need: usize, burst: usize, gap: usize, catch_up: usize) {
        let flow = |c: &mut Cushion, frames: usize, first_extra: usize| {
            let (mut t, mut next, mut extra) = (0, 0, first_extra);
            while t < frames {
                let mut arrived = 0;
                while next <= t {
                    arrived += burst + std::mem::take(&mut extra);
                    next += burst;
                }
                c.arrived(arrived);
                t += need;
            }
        };
        flow(c, SR as usize, 0);
        c.underrun(need);
        for _ in 0..gap / need {
            c.arrived(0);
        }
        flow(c, SR as usize, catch_up);
    }

    #[test]
    fn late_audio_deepens_the_queue_once_by_what_was_late() {
        let mut c = Cushion::new(32, SR);
        c.prime(c.start_level()).expect("primed");
        let before = c.target();
        // 20 ms held back, then delivered all at once with the next burst.
        gap_then(&mut c, 32, 960, 960, 960);
        let grown = c.target() - before;
        assert!(grown >= 900, "depth must cover the lateness: +{grown}");
        assert!(grown < 2 * 1_920, "one outage, not thirty: +{grown}");
    }

    #[test]
    fn a_source_going_quiet_is_not_charged_as_jitter() {
        let mut c = Cushion::new(64, SR);
        c.prime(c.start_level()).expect("primed");
        let before = c.target();
        // Five seconds of a paused app, then it plays on at the ordinary rate.
        gap_then(&mut c, 64, 480, 5 * SR as usize, 0);
        assert_eq!(c.target(), before, "a pause left the queue deeper");
        // And a short quiet spell between sounds, the same.
        gap_then(&mut c, 64, 480, 384, 0);
        assert_eq!(c.target(), before, "a 8 ms gap left the queue deeper");
    }

    #[test]
    fn the_target_always_fits_the_input_ring() {
        let mut c = Cushion::new(64, SR);
        for _ in 0..50 {
            // Huge lateness, all caught up: the worst a capture can report.
            gap_then(&mut c, 64, 480, 320_000, 320_000);
        }
        assert!(c.target() < SR as usize / 2, "target {} frames", c.target());
    }

    #[test]
    fn an_app_that_plays_and_pauses_keeps_capture_latency() {
        // Sounds of 200 ms with 60-400 ms of nothing between them, for two
        // minutes: none of that silence may turn into queue depth.
        let quiet: Vec<(f64, f64)> = (0..240)
            .map(|k| {
                let start = k as f64 * 0.5 + 0.2;
                (start, start + 0.06 + (k % 7) as f64 * 0.05)
            })
            .collect();
        let steady = simulate(64, 480, 0.0, 0.0, 0, 120.0, 10.0);
        let r = simulate_with_quiet(64, 480, 0.0, 0.0, 0, 120.0, 10.0, &quiet);
        assert_eq!(r.late_underruns, 0, "underran while the app was playing");
        // One delivery of overshoot on resuming is the onset being played,
        // not cut; anything beyond is silence turned into latency.
        assert!(
            r.max_queued <= steady.max_queued + 480,
            "pauses deepened the queue: {} vs {} frames",
            r.max_queued,
            steady.max_queued
        );
    }

    #[test]
    fn jitter_still_buys_headroom_between_pauses() {
        // A jittery capture that also pauses: the jitter is still measured.
        let quiet: Vec<(f64, f64)> = (0..60)
            .map(|k| (k as f64 * 2.0 + 1.0, k as f64 * 2.0 + 1.3))
            .collect();
        let r = simulate_with_quiet(64, 480, 0.0, 0.8, 0, 120.0, 30.0, &quiet);
        assert_eq!(r.late_underruns, 0);
    }

    /// Audible events a listener would notice, after settling.
    #[derive(Debug, Default)]
    struct Heard {
        splices: usize,
        underruns: usize,
        mean_queued: f64,
    }

    /// Plays `deliveries` (time in seconds, frames) through a cushion read
    /// in `block`-frame blocks at 44.1 kHz.
    fn replay(block: usize, deliveries: &[(f64, usize)], seconds: f64, settle: f64) -> Heard {
        replay_with(
            Cushion::new(block, 44_100),
            block,
            deliveries,
            seconds,
            settle,
        )
    }

    fn replay_with(
        mut c: Cushion,
        block: usize,
        deliveries: &[(f64, usize)],
        seconds: f64,
        settle: f64,
    ) -> Heard {
        let rate = 44_100u32;
        let period = block as f64 / rate as f64;
        let (mut t, mut i, mut queued, mut arrived) = (0.0, 0usize, 0usize, 0usize);
        let mut heard = Heard::default();
        let mut samples = 0usize;
        while t < seconds {
            while i < deliveries.len() && deliveries[i].0 <= t {
                queued += deliveries[i].1;
                arrived += deliveries[i].1;
                i += 1;
            }
            c.arrived(std::mem::take(&mut arrived));
            let counting = t >= settle;
            if !c.is_primed() {
                match c.prime(queued) {
                    Some(excess) => queued -= excess,
                    None => {
                        t += period;
                        continue;
                    }
                }
            }
            match c.observe(queued) {
                Adjust::Drop(n) => {
                    queued -= n;
                    heard.splices += counting as usize;
                }
                Adjust::Insert(n) => {
                    queued += n;
                    heard.splices += counting as usize;
                }
                Adjust::None => {}
            }
            if queued < block {
                heard.underruns += counting as usize;
                c.underrun(block - queued);
                queued = 0;
            } else {
                queued -= block;
            }
            if counting {
                heard.mean_queued += queued as f64;
                samples += 1;
            }
            t += period;
        }
        heard.mean_queued /= samples.max(1) as f64;
        heard
    }

    /// A Core Audio process tap as it reaches a graph: 512-frame buffers on
    /// the output device's clock, handed on by the normalizer thread in
    /// 256-frame pieces after a poll of up to 1 ms, and every few seconds a
    /// scheduling stall of 5-20 ms after which the backlog arrives at once.
    fn tap_through_normalizer(seconds: f64, seed: u32) -> Vec<(f64, usize)> {
        let rate = 44_100.0;
        let mut rng = seed;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng % 10_000) as f64 / 10_000.0
        };
        let mut out = Vec::new();
        let mut stall_until = 0.0;
        let mut k = 0usize;
        loop {
            let captured = (k + 1) as f64 * 512.0 / rate;
            if captured > seconds {
                break;
            }
            if next() < 0.004 {
                stall_until = captured + 0.005 + next() * 0.015;
            }
            for half in 0..2 {
                let poll = next() * 0.001;
                let at = (captured + poll).max(stall_until) + half as f64 * 0.00002;
                out.push((at, 256));
            }
            k += 1;
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    // The chain in the field report: app audio into a speaker at 64 frames.
    // The source runs on the speaker's own clock, so there is nothing to
    // correct; every splice or dropout is one the listener hears.
    #[test]
    fn a_process_tap_on_the_output_clock_plays_clean() {
        for seed in [1, 7, 42] {
            let deliveries = tap_through_normalizer(180.0, seed);
            let h = replay(64, &deliveries, 180.0, 20.0);
            assert!(
                h.splices + h.underruns <= 2,
                "seed {seed}: heard {} splices and {} dropouts in 160 s",
                h.splices,
                h.underruns
            );
        }
    }

    /// Recorded from a live Core Audio process tap on Chrome at 44.1 kHz with
    /// the HAL's default 512-frame buffer: steady 11.6 ms deliveries, one
    /// 30 ms stall caught up by a 1536-frame delivery, a 1 s pause at 12 s and
    /// the app stopping at 23 s.
    fn recorded_chrome_tap() -> Vec<(f64, usize)> {
        include_str!("testdata/chrome_tap_512.csv")
            .lines()
            .map(|l| {
                let (t, n) = l.split_once(',').expect("time,frames");
                (t.parse().expect("time"), n.parse().expect("frames"))
            })
            .collect()
    }

    // The field report, on real delivery timing: a tap on the speaker's own
    // clock must play without a single splice. Dropouts are allowed only
    // where the app itself went quiet or stalled.
    #[test]
    fn recorded_chrome_tap_plays_without_splices_when_locked() {
        let trace = recorded_chrome_tap();
        // Counting from 2 s: the one startup correction lands in the first.
        for block in [32, 64, 128, 256] {
            let h = replay_with(Cushion::locked(block, 44_100), block, &trace, 23.0, 2.0);
            assert_eq!(h.splices, 0, "{block}: {h:?}");
            assert!(h.underruns <= 2, "{block}: {h:?}");
            // About one delivery plus its jitter, well under the 28 ms the
            // level-chasing cushion held on this trace.
            assert!(h.mean_queued < 44.1 * 20.0, "{block}: {h:?}");
        }
    }

    /// The same tap with its aggregate asked for 64-frame buffers: one
    /// delivery every 1.45 ms instead of every 11.6 ms.
    fn recorded_chrome_tap_64() -> Vec<(f64, usize)> {
        include_str!("testdata/chrome_tap_64.csv")
            .lines()
            .map(|l| {
                let (t, n) = l.split_once(',').expect("time,frames");
                (t.parse().expect("time"), n.parse().expect("frames"))
            })
            .collect()
    }

    #[test]
    fn a_small_tap_buffer_brings_the_queue_down_to_a_few_ms() {
        let trace = recorded_chrome_tap_64();
        let end = trace.last().expect("rows").0;
        for block in [32, 64, 128] {
            let h = replay_with(Cushion::locked(block, 44_100), block, &trace, end, 2.0);
            assert_eq!(h.splices, 0, "{block}: {h:?}");
            // The recording holds one real tap stall (a 320-frame catch-up
            // after ~6 ms of nothing): a lone stall costs one dropout rather
            // than latency for the rest of the session.
            assert!(h.underruns <= 1, "{block}: {h:?}");
            // Two engine blocks of floor, then ~5 ms for the tap's own buffer,
            // its jitter and the safety margin.
            let bound = 2.0 * block as f64 + 44.1 * 5.0;
            assert!(h.mean_queued < bound, "{block}: {h:?}");
        }
    }

    #[test]
    fn a_locked_source_realigns_only_from_silence() {
        let mut c = Cushion::locked(64, SR);
        let start = c.prime(20_000).expect("started");
        assert!(start > 0, "startup backlog goes");
        // Playing along: nothing is ever corrected.
        for q in [100, 5_000, 20_000, 700] {
            assert_eq!(c.observe(q), Adjust::None);
        }
        // Ran dry, then late audio caught up: it realigns under the silence,
        // dropping what arrived for time already played as silence.
        c.underrun(64);
        let level = c.start_level();
        assert_eq!(c.prime(level + 1_536), Some(1_536));
    }

    #[test]
    fn only_the_first_start_discards() {
        let mut c = Cushion::new(64, SR);
        assert!(
            c.prime(20_000).expect("started") > 0,
            "startup backlog goes"
        );
        c.underrun(64);
        assert_eq!(c.prime(20_000), Some(0), "a refill keeps what arrived");
    }

    #[test]
    fn a_splice_waits_until_the_queue_can_afford_it() {
        let mut c = Cushion::new(64, SR);
        c.owed = -64;
        assert_eq!(c.observe(64), Adjust::None, "not enough queued to cut");
        assert_eq!(c.observe(64 + 64 + 2 * 32), Adjust::Drop(64));
    }
}

/// Records a live tap's delivery timing (seconds,frames per line) to
/// `TAP_OUT`, the source of `testdata/chrome_tap_512.csv`. Needs Chrome playing
/// and the System Audio Recording permission; `TAP_FRAMES` sets its buffer.
#[cfg(all(test, target_os = "macos"))]
mod tap_recorder {
    use std::io::Write;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "records a live Chrome tap"]
    fn record_chrome_tap() {
        let (mut tx, rx) = crate::audio::input_bridge::broadcast_channel();
        let (prod, cons) = rtrb::RingBuffer::<f32>::new(2_000_000);
        tx.add(prod).unwrap();
        let io_frames = std::env::var("TAP_FRAMES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let cap =
            crate::audio::capture::Capture::start_app("com.google.Chrome", 44_100, io_frames, rx)
                .expect("tap");
        let start = Instant::now();
        let mut rows = Vec::new();
        let mut seen = 0usize;
        while start.elapsed() < Duration::from_secs(30) {
            let n = cons.slots();
            if n > seen {
                rows.push((start.elapsed().as_secs_f64(), (n - seen) / 2));
                seen = n;
            }
            std::thread::sleep(Duration::from_micros(100));
        }
        drop(cap);
        let path = std::env::var("TAP_OUT").unwrap();
        let mut f = std::fs::File::create(path).unwrap();
        for (t, n) in &rows {
            writeln!(f, "{t:.6},{n}").unwrap();
        }
        println!("rows {} rate {}", rows.len(), seen as f64 / 2.0 / 30.0);
    }
}
