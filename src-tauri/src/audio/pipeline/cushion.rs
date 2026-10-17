//! How much live audio a source keeps queued ahead of the graph.
//!
//! A capture device hands audio over in bursts on its own clock while the
//! graph takes one block per output callback, so the queue saws up and down.
//! `DepthEstimator` sizes the headroom from that saw and the delivery jitter
//! on top of it. `Cushion` decides when the source plays: it holds silence
//! until the queue reaches its start level, and after running dry it realigns
//! under the silence it is already playing, never while audio is heard.
//!
//! It never corrects the queue while the source plays, with one exception:
//! the depth guessed before anything was measured is corrected once, by one
//! crossfaded cut, as soon as it is measured. A source rebuilt by an edit
//! starts from the depth its predecessor measured (`DepthMemo`) and has
//! nothing to correct. Drift between clocks is the resampler's
//! to absorb (`asrc`), and it holds still until that correction is done, so
//! the two never act on the queue at once. A source on the output's own clock
//! reads without one.
//!
//! Running dry deepens the queue only by audio that turns out to have been
//! late (`OutageJudge`), and only when it happens again within a minute: a
//! source that goes quiet leaves the depth where it was, and a lone stall
//! costs one dropout rather than latency for everything after it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::audio::adaptive_depth::{DepthEstimator, DepthMemo, OutageJudge};

/// Correction the source should apply before reading its next block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Adjust {
    None,
    /// Cut `frames` out of the queue, crossfading the `fade` frames before
    /// the cut into the `fade` after it.
    Drop {
        frames: usize,
        fade: usize,
    },
}

/// Longest crossfade the startup correction cuts under, when the queue holds
/// that much past the cut. One long blend is heard as a single soft
/// transition, where many short splices crackle.
const STARTUP_FADE_MS: f64 = 10.0;

/// `STARTUP_FADE_MS` in frames at `rate`.
pub(super) const fn startup_fade_frames(rate: u32) -> usize {
    let frames = (rate as f64 * STARTUP_FADE_MS / 1000.0 + 0.5) as usize;
    if frames == 0 {
        1
    } else {
        frames
    }
}

/// Every source's memo, by what makes it the same source: the output it
/// plays in and the input it reads. Lives as long as the pipeline does.
#[derive(Default)]
pub(super) struct DepthMemos(Mutex<HashMap<String, Arc<DepthMemo>>>);

impl DepthMemos {
    pub(super) fn get(&self, key: &str) -> Arc<DepthMemo> {
        self.0
            .lock()
            .unwrap()
            .entry(key.to_string())
            .or_default()
            .clone()
    }
}

/// One dip measurement spans this long; it has to cover the slowest capture
/// burst (Bluetooth and ScreenCaptureKit deliver every ~20 ms).
const WINDOW_MS: f64 = 250.0;
/// Headroom assumed before the first deliveries have been seen. Once they
/// have, the guess is their size (see `arrived`).
const PRIOR_MS: f64 = 10.0;
/// How fast the saw's height sinks back once deliveries get smaller.
const QUANTUM_RELEASE_S: f64 = 30.0;
/// Kept above the measured dip at all times. A process tap's p99.9 delivery
/// jitter is already ~1.8 ms, so a thinner margin clicks every few seconds.
const SAFETY_MS: f64 = 2.0;
/// When the queue cannot hold the whole startup cut at once, the next part
/// waits this long.
const SPLICE_EVERY_MS: f64 = 21.0;
/// Ceiling on the measured headroom. Jitter beyond this is a broken capture,
/// and a deeper target would outgrow the one-second input ring and never
/// finish priming.
const MAX_DEPTH_MS: f64 = 250.0;
/// A late delivery deepens the queue only if another came within this long.
const LATE_REPEAT_MS: f64 = 60_000.0;

pub(super) struct Cushion {
    need: usize,
    safety: usize,
    depth: DepthEstimator,
    sum: f64,
    blocks: usize,
    low: usize,
    /// Startup correction still owed, as frames to drop.
    owed: usize,
    cooldown: usize,
    cooldown_blocks: usize,
    /// Longest and shortest crossfade of the startup cut, in frames.
    max_fade: usize,
    min_fade: usize,
    primed: bool,
    /// The queue starts from the prior; the first measurement corrects it
    /// once, the way a startup offset is removed, and never again.
    settled: bool,
    judge: OutageJudge,
    max_depth: usize,
    /// Blocks since the last late delivery was seen, for `LATE_REPEAT_MS`.
    since_late: usize,
    late_repeat_blocks: usize,
    /// Largest recent arrival between two reads: the saw's height.
    quantum: f64,
    /// Share of `quantum` kept per block, for `QUANTUM_RELEASE_S`.
    quantum_keep: f64,
    /// A delivery has arrived since the queue was created.
    seen_delivery: bool,
    /// Primed at least once.
    started: bool,
    rate: u32,
    memo: Arc<DepthMemo>,
    /// Depth recalled from `memo`: the prior never drops below it.
    recalled: usize,
}

impl Cushion {
    /// `need` is the frames the graph takes per block, in the queue's rate.
    pub(super) fn new(need: usize, sample_rate: u32, memo: Arc<DepthMemo>) -> Self {
        let frames = |ms: f64| ((sample_rate as f64 * ms / 1000.0).round() as usize).max(1);
        let recalled = memo.recall(sample_rate);
        let mut c = Self {
            need,
            safety: frames(SAFETY_MS).max(16),
            depth: DepthEstimator::new(sample_rate, need, WINDOW_MS, frames(PRIOR_MS)),
            sum: 0.0,
            blocks: 0,
            low: usize::MAX,
            owed: 0,
            cooldown: 0,
            cooldown_blocks: (frames(SPLICE_EVERY_MS) / need.max(1)).max(1),
            max_fade: startup_fade_frames(sample_rate),
            min_fade: super::dag::splice_fade_frames(sample_rate),
            primed: false,
            settled: false,
            judge: OutageJudge::new(sample_rate),
            max_depth: frames(MAX_DEPTH_MS),
            since_late: usize::MAX,
            late_repeat_blocks: frames(LATE_REPEAT_MS) / need.max(1),
            quantum: 0.0,
            quantum_keep: (-(need.max(1) as f64) / sample_rate.max(1) as f64 / QUANTUM_RELEASE_S)
                .exp(),
            seen_delivery: false,
            started: false,
            rate: sample_rate,
            memo,
            recalled: 0,
        };
        if let Some((dip, quantum)) = recalled {
            c.recalled = dip;
            c.depth.set_prior(dip);
            c.quantum = quantum as f64;
            c.settled = true;
        }
        c
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

    fn quantum(&self) -> usize {
        self.quantum.round() as usize
    }

    /// Once per block, before anything else: the frames the source delivered
    /// since the last block. A gap's verdict lands here, once the flow after
    /// it shows whether the missing audio was late or never existed.
    pub(super) fn arrived(&mut self, frames: usize) {
        self.since_late = self.since_late.saturating_add(1);
        // The largest single arrival is how far the queue saws between reads;
        // it sinks back over `QUANTUM_RELEASE_S` if deliveries get smaller.
        // Only steady flow counts: a catch-up burst after a gap is the gap,
        // not the saw. While first filling, the first delivery may be
        // backlog from before the first read; later ones are the saw, and
        // until the dip is measured a whole saw of them stands in for it:
        // twice an ideal saw's dip, room for jitter not yet measured.
        self.quantum *= self.quantum_keep;
        let steady = if self.primed {
            self.judge.is_clear()
        } else {
            !self.started && self.seen_delivery
        };
        if frames > 0 {
            self.seen_delivery = true;
        }
        if steady && frames > 0 {
            let cap = if self.quantum > 0.0 {
                2.0 * self.quantum
            } else {
                f64::INFINITY
            };
            self.quantum = self.quantum.max((frames as f64).min(cap));
            if !self.started {
                self.depth.set_prior(self.quantum().max(self.recalled));
            }
        }
        if let Some(late) = self.judge.delivered(frames, self.need) {
            if late > 0 {
                if self.since_late <= self.late_repeat_blocks {
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
    pub(super) fn start_level(&self) -> usize {
        let dip = self.depth.depth().min(self.max_depth);
        let quantum = self.quantum();
        if quantum == 0 {
            return self.target() + dip;
        }
        let jitter = dip.saturating_sub(quantum / 2);
        self.floor() + quantum.max(dip) + jitter
    }

    /// Before the source's first read, and again after it ran dry. `None`
    /// while the queue is still filling: play silence for this block.
    ///
    /// Returns the frames to discard: anything beyond the start level. On the
    /// first start that is backlog nobody has heard; after running dry it is
    /// audio that arrived late for time already played as silence, and
    /// dropping exactly that, under the silence, keeps the latency where it was.
    pub(super) fn prime(&mut self, queued: usize) -> Option<usize> {
        let start = self.start_level();
        if queued < start {
            return None;
        }
        self.primed = true;
        self.started = true;
        self.owed = 0;
        self.sum = 0.0;
        self.blocks = 0;
        self.low = usize::MAX;
        // Deliveries right after a gap come in catch-up bursts; measuring them
        // would read the gap's aftermath as the source's everyday jitter.
        self.depth.discard_window();
        Some(queued - start)
    }

    pub(super) fn is_primed(&self) -> bool {
        self.primed
    }

    /// The startup correction is done: from here on only the resampler moves
    /// the queue.
    pub(super) fn is_settled(&self) -> bool {
        self.settled && self.owed == 0
    }

    /// The graph ran `missing` frames dry mid-block: refill before playing
    /// on, rather than limp along a starved queue one click per block.
    pub(super) fn underrun(&mut self, missing: usize) {
        self.primed = false;
        self.judge.missing(missing);
        // The window holds the drain, which says nothing about jitter.
        self.depth.discard_window();
    }

    /// The output stopped reading for a while (a device overload skips its
    /// callbacks) while the source delivered on. What arrived meanwhile is
    /// for time the listener already lost to that dropout: realign under it,
    /// as after any silence, instead of keeping it queued as latency for
    /// good. Not the source's fault, so its depth is left as it was.
    pub(super) fn output_stalled(&mut self) {
        self.primed = false;
        self.depth.discard_window();
    }

    /// Call once per block with the frames queued before the read. Returns the
    /// startup correction's next splice, if one is due.
    pub(super) fn observe(&mut self, queued: usize) -> Adjust {
        // While the correction runs the queue moves by design, and a window
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
            if self.depth.observe(queued) {
                let mean = self.sum / self.blocks as f64;
                let low = self.low;
                self.sum = 0.0;
                self.blocks = 0;
                self.low = usize::MAX;
                if self.depth.is_measured() {
                    self.memo
                        .keep(self.depth.depth(), self.quantum(), self.rate);
                    if !self.settled {
                        self.settled = true;
                        self.owed = self.startup_excess(mean, low);
                    }
                }
            }
        }
        if self.cooldown > 0 {
            self.cooldown -= 1;
            return Adjust::None;
        }
        if self.owed > 0 {
            // The cut has to leave this block's read and the shortest fade
            // behind; the fade then takes whatever the queue holds past the
            // cut, up to the longest.
            let n = self
                .owed
                .min(queued.saturating_sub(self.need + self.min_fade));
            if n > 0 {
                self.owed -= n;
                if self.owed > 0 {
                    self.cooldown = self.cooldown_blocks;
                }
                let fade = (queued - n).min(self.max_fade);
                return Adjust::Drop { frames: n, fade };
            }
        }
        Adjust::None
    }

    /// How far the first measured window sat above target, never cutting the
    /// window's emptiest point below the floor.
    fn startup_excess(&self, mean: f64, low: usize) -> usize {
        let over = mean - self.target() as f64;
        let slack = (self.depth.depth() / 4).max(self.safety) as f64;
        if over <= slack {
            return 0;
        }
        over.min(low.saturating_sub(self.floor()) as f64).round() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::drift_loop::{ArrivalClock, DriftLoop};

    const SR: u32 = 48_000;

    /// Audible events a listener would notice, after settling.
    #[derive(Debug, Default)]
    struct Heard {
        splices: usize,
        underruns: usize,
        mean_queued: f64,
        max_queued: usize,
    }

    /// How the graph reads the queue: a block at a time, either straight (a
    /// source on the output's clock) or through the drift resampler, which
    /// takes `block * (1 + u)` with `u` from the loop.
    #[derive(Clone, Copy, PartialEq)]
    enum Read {
        Locked,
        Steered,
    }

    /// Plays `deliveries` (time in seconds, frames) through a cushion read in
    /// `block`-frame blocks. Dropouts inside a `quiet` span (give or take
    /// 50 ms) are the source's own silence and are not counted.
    #[allow(clippy::too_many_arguments)]
    fn play(
        mut c: Cushion,
        rate: u32,
        block: usize,
        read: Read,
        deliveries: &[(f64, usize)],
        seconds: f64,
        settle: f64,
        quiet: &[(f64, f64)],
    ) -> Heard {
        let period = block as f64 / rate as f64;
        let mut drift = DriftLoop::new(rate, block);
        let mut arrival = ArrivalClock::new(rate);
        let (mut t, mut i, mut queued, mut arrived, mut total) = (0.0, 0, 0, 0, 0u64);
        let mut frac = 0.0;
        let mut steering = false;
        let mut heard = Heard::default();
        let mut samples = 0usize;
        while t < seconds {
            while i < deliveries.len() && deliveries[i].0 <= t {
                let (at, n) = deliveries[i];
                queued += n;
                arrived += n;
                total += n as u64;
                arrival.observe(total, at);
                i += 1;
            }
            c.arrived(std::mem::take(&mut arrived));
            let quiet_now = quiet.iter().any(|&(a, b)| t >= a - 0.05 && t < b + 0.05);
            let counting = t >= settle && !quiet_now;
            if !c.is_primed() {
                match c.prime(queued) {
                    Some(excess) => {
                        queued -= excess;
                        drift.restart();
                        arrival.reset();
                    }
                    None => {
                        t += period;
                        continue;
                    }
                }
            }
            if let Adjust::Drop { frames: n, .. } = c.observe(queued) {
                queued -= n;
                heard.splices += counting as usize;
            }
            let take = match read {
                Read::Locked => block,
                Read::Steered if !c.is_settled() => {
                    steering = false;
                    block
                }
                Read::Steered => {
                    if !steering {
                        steering = true;
                        drift.restart();
                    }
                    let start = c.start_level() as f64;
                    let u = drift.update(queued as f64 + arrival.pending(t, start) - start);
                    frac += block as f64 * (1.0 + u);
                    let take = frac as usize;
                    frac -= take as f64;
                    take
                }
            };
            if queued < take {
                heard.underruns += counting as usize;
                c.underrun(take - queued);
                queued = 0;
            } else {
                queued -= take;
            }
            if counting {
                heard.mean_queued += queued as f64;
                heard.max_queued = heard.max_queued.max(queued);
                samples += 1;
            }
            t += period;
        }
        heard.mean_queued /= samples.max(1) as f64;
        heard
    }

    /// A capture device delivering `burst` frames at a clock `capture_ppm`
    /// off ours, each delivery late by a pseudo-random share `jitter` of its
    /// period, nothing inside a `quiet` span, and `initial_backlog` frames
    /// waiting at the start.
    fn capture(
        burst: usize,
        capture_ppm: f64,
        jitter: f64,
        initial_backlog: usize,
        seconds: f64,
        quiet: &[(f64, f64)],
    ) -> Vec<(f64, usize)> {
        let in_period = burst as f64 / (SR as f64 * (1.0 + capture_ppm * 1e-6));
        let mut seed: u32 = 0x2545_f491;
        let mut out = vec![(0.0, initial_backlog)];
        let mut next = in_period * 0.37;
        while next < seconds {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let late = jitter * in_period * (seed % 1000) as f64 / 1000.0;
            if !quiet.iter().any(|&(a, b)| next >= a && next < b) {
                out.push((next + late, burst));
            }
            next += in_period;
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    fn simulate(
        block: usize,
        burst: usize,
        capture_ppm: f64,
        jitter: f64,
        initial_backlog: usize,
        seconds: f64,
        settle_s: f64,
    ) -> Heard {
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
    ) -> Heard {
        let deliveries = capture(burst, capture_ppm, jitter, initial_backlog, seconds, quiet);
        play(
            Cushion::new(block, SR, Arc::default()),
            SR,
            block,
            Read::Steered,
            &deliveries,
            seconds,
            settle_s,
            quiet,
        )
    }

    #[test]
    fn startup_backlog_is_dropped_before_anything_plays() {
        let mut c = Cushion::new(64, SR, Arc::default());
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
            assert_eq!(r.underruns, 0, "{block}/{burst}: underruns once settled");
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
        // 500 ppm is far beyond real crystal error; 100 s drifts ~2400 frames,
        // all of which the resampler's ratio has to absorb without a splice.
        for block in [32, 64, 256, 1024] {
            for burst in [256, 512, 960] {
                for ppm in [-500.0, -50.0, 50.0, 500.0] {
                    let r = simulate(block, burst, ppm, 0.0, 0, 100.0, 10.0);
                    let case = format!("{block}/{burst} @ {ppm} ppm");
                    assert_eq!(r.underruns, 0, "{case}: underran");
                    assert_eq!(r.splices, 0, "{case}: spliced");
                    let bound = burst + 3 * block + 2 * 48 + 256;
                    assert!(r.max_queued < bound, "{case}: max queue {}", r.max_queued);
                }
            }
        }
    }

    #[test]
    fn jittery_capture_buys_headroom_instead_of_glitching() {
        // A loaded VM: every burst lands up to 80% of its period late.
        let r = simulate(64, 480, 0.0, 0.8, 0, 120.0, 30.0);
        assert_eq!(r.underruns, 0);
        assert!(r.mean_queued < 3.0 * 480.0, "mean queue {}", r.mean_queued);
    }

    #[test]
    fn nothing_is_spliced_after_the_startup_correction() {
        // The one correction of the guessed start depth runs within the first
        // two seconds; after that nothing is ever cut.
        for ppm in [-100.0, 0.0, 100.0] {
            let deliveries = capture(512, ppm, 0.3, 4_800, 60.0, &[]);
            let c = Cushion::new(64, SR, Arc::default());
            let r = play(c, SR, 64, Read::Steered, &deliveries, 60.0, 2.0, &[]);
            assert_eq!(r.splices, 0, "{ppm} ppm");
        }
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
        let mut c = Cushion::new(32, SR, Arc::default());
        c.prime(c.start_level()).expect("primed");
        let before = c.target();
        // 20 ms held back, then delivered all at once with the next burst. A
        // lone stall costs its dropout and nothing more.
        gap_then(&mut c, 32, 960, 960, 960);
        assert_eq!(c.target(), before, "one stall deepened the queue");
        // The same again within a minute: this source does stall, so the
        // queue covers it from now on.
        gap_then(&mut c, 32, 960, 960, 960);
        let grown = c.target() - before;
        assert!(grown >= 900, "depth must cover the lateness: +{grown}");
        assert!(grown < 2 * 1_920, "one outage, not thirty: +{grown}");
    }

    #[test]
    fn a_source_going_quiet_is_not_charged_as_jitter() {
        let mut c = Cushion::new(64, SR, Arc::default());
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
        let mut c = Cushion::new(64, SR, Arc::default());
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
        assert_eq!(r.underruns, 0, "underran while the app was playing");
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
        assert_eq!(r.underruns, 0);
    }

    fn replay(
        block: usize,
        read: Read,
        deliveries: &[(f64, usize)],
        seconds: f64,
        settle: f64,
    ) -> Heard {
        let c = Cushion::new(block, 44_100, Arc::default());
        play(c, 44_100, block, read, deliveries, seconds, settle, &[])
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
            let h = replay(64, Read::Steered, &deliveries, 180.0, 20.0);
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
            let h = replay(block, Read::Locked, &trace, 23.0, 2.0);
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
            let h = replay(block, Read::Locked, &trace, end, 2.0);
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
    fn the_queue_realigns_only_from_silence() {
        let mut c = Cushion::new(64, SR, Arc::default());
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
    fn small_deliveries_start_below_the_prior() {
        let mut c = Cushion::new(32, SR, Arc::default());
        let guessed = c.start_level();
        for _ in 0..4 {
            c.arrived(32);
        }
        assert!(
            c.start_level() < guessed / 2,
            "{} vs {guessed}",
            c.start_level()
        );
    }

    #[test]
    fn the_saw_height_sinks_back_at_the_same_pace_at_any_block() {
        let sunk = |need: usize| {
            let mut c = Cushion::new(need, SR, Arc::default());
            c.arrived(512);
            c.arrived(512);
            for _ in 0..(10 * SR as usize) / need {
                c.arrived(0);
            }
            c.quantum
        };
        let (small, large) = (sunk(32), sunk(1_024));
        assert!((small - large).abs() < 1.0, "{small} vs {large}");
    }

    #[test]
    fn a_splice_waits_until_the_queue_can_afford_it() {
        let mut c = Cushion::new(64, SR, Arc::default());
        c.owed = 64;
        assert_eq!(c.observe(64), Adjust::None, "not enough queued to cut");
        assert_eq!(
            c.observe(64 + 64 + 2 * 32),
            Adjust::Drop {
                frames: 64,
                fade: 128
            }
        );
    }

    #[test]
    fn the_startup_correction_is_one_cut_under_a_long_fade() {
        let mut c = Cushion::new(64, SR, Arc::default());
        c.owed = 960;
        assert_eq!(
            c.observe(4_000),
            Adjust::Drop {
                frames: 960,
                fade: startup_fade_frames(SR)
            }
        );
        assert_eq!(c.observe(4_000), Adjust::None, "nothing left owed");
    }

    #[test]
    fn a_source_rebuilt_by_an_edit_starts_at_the_measured_depth() {
        // The first source measures its delivery and corrects its guessed
        // start once; the one replacing it starts where that left off and
        // never cuts.
        let memo: Arc<DepthMemo> = Arc::default();
        let deliveries = capture(512, 80.0, 0.3, 4_800, 6.0, &[]);
        let first = Cushion::new(64, SR, memo.clone());
        let r = play(first, SR, 64, Read::Steered, &deliveries, 6.0, 0.0, &[]);
        assert!(r.splices > 0, "the first start corrects its guess");
        let second = Cushion::new(64, SR, memo);
        let r = play(second, SR, 64, Read::Steered, &deliveries, 6.0, 0.0, &[]);
        assert_eq!(r.splices, 0, "the rebuilt source cuts");
        assert_eq!(r.underruns, 0, "the rebuilt source runs dry");
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
