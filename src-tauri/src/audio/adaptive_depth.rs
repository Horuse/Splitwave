//! Adaptive queue depth for audio that arrives on one clock and is consumed on
//! another: captured input read by a speaker graph, network audio read by any
//! output.
//!
//! What a queue needs is headroom against its own jitter: how far it dips
//! below its typical level before the next delivery lands. Measuring that dip
//! rather than the level itself is the point. The level is what the caller's
//! corrections move, so a target derived from the level chases its own
//! corrections; the dip (window mean minus window low) is the same whatever
//! level the queue is held at.
//!
//! Dips are kept in a histogram with exponential forgetting and the depth is
//! a high quantile of it, the way WebRTC's NetEq sizes its jitter buffer. An
//! underrun is recorded as an outage large enough to move the quantile at
//! once (fast attack); forgetting hands unused depth back over about a minute
//! (slow release), so a single spike cannot pin the latency high and a
//! repeating one is not forgotten between repeats.
//!
//! Running dry is not always lateness. A source that goes quiet (a paused
//! app, a sender with nothing to say) leaves the queue empty too, but that
//! audio does not exist and never arrives. `OutageJudge` tells the two apart
//! by what follows: late audio is still owed by a clocked source, so it turns
//! up as delivery running ahead of real time once the flow resumes; silence
//! is followed by delivery at the ordinary rate. Only what was caught up is
//! lateness worth buffering against.
//!
//! Everything is fixed-size: every method here is RT-safe.

const BUCKETS: usize = 64;
/// First non-zero bucket edge in frames; each next edge is 20% wider.
const FIRST_EDGE: f64 = 16.0;
const GROWTH: f64 = 1.2;
/// Share of the dip history the depth has to cover.
const QUANTILE: f64 = 0.99;
/// How long an observation keeps half its weight.
const HALF_LIFE_MS: f64 = 45_000.0;
/// An outage takes this share of the whole history at once, well above
/// 1 - QUANTILE, so the very next depth covers it.
const OUTAGE_SHARE: f64 = 0.05;
/// Measured windows before the prior stops counting.
const PRIOR_WINDOWS: usize = 4;

fn edge(i: usize) -> usize {
    if i == 0 {
        0
    } else {
        (FIRST_EDGE * GROWTH.powi(i as i32 - 1)).round() as usize
    }
}

fn bucket(frames: usize) -> usize {
    (1..BUCKETS)
        .find(|&i| frames < edge(i))
        .map_or(BUCKETS - 1, |i| i - 1)
}

pub struct DepthEstimator {
    weights: [f64; BUCKETS],
    total: f64,
    /// Depth assumed until `PRIOR_WINDOWS` windows have been measured.
    prior: usize,
    measured: usize,
    forget: f64,
    window_blocks: usize,
    blocks: usize,
    sum: f64,
    low: usize,
}

impl DepthEstimator {
    /// `block_frames` are consumed per `observe`; `window_ms` is how much time
    /// one dip measurement spans (it must cover the slowest delivery burst).
    /// `prior_frames` is the dip assumed before anything has been measured.
    pub fn new(sample_rate: u32, block_frames: usize, window_ms: f64, prior_frames: usize) -> Self {
        let window_frames = sample_rate as f64 * window_ms / 1000.0;
        let window_blocks = ((window_frames / block_frames.max(1) as f64).round() as usize).max(4);
        let window_ms = window_blocks as f64 * block_frames as f64 * 1000.0 / sample_rate as f64;
        Self {
            weights: [0.0; BUCKETS],
            total: 0.0,
            prior: prior_frames,
            measured: 0,
            forget: 0.5_f64.powf(window_ms / HALF_LIFE_MS),
            window_blocks,
            blocks: 0,
            sum: 0.0,
            low: usize::MAX,
        }
    }

    /// Frames queued just before a block was taken. Closes a window every
    /// `window_blocks` calls; returns true when it did.
    pub fn observe(&mut self, queued: usize) -> bool {
        self.sum += queued as f64;
        self.low = self.low.min(queued);
        self.blocks += 1;
        if self.blocks < self.window_blocks {
            return false;
        }
        let mean = self.sum / self.blocks as f64;
        let dip = (mean - self.low as f64).max(0.0).round() as usize;
        self.blocks = 0;
        self.sum = 0.0;
        self.low = usize::MAX;
        self.measured += 1;
        self.record(dip, 1.0);
        true
    }

    /// The queue ran out `deficit` frames short of what the consumer needed,
    /// on top of whatever depth it was already holding.
    pub fn underrun(&mut self, deficit: usize) {
        let dip = self.depth() + deficit;
        let weight = (self.total * OUTAGE_SHARE / (1.0 - OUTAGE_SHARE)).max(1.0);
        self.record(dip, weight);
        self.blocks = 0;
        self.sum = 0.0;
        self.low = usize::MAX;
    }

    /// True once the depth comes from measurement rather than the prior.
    pub fn is_measured(&self) -> bool {
        self.measured >= PRIOR_WINDOWS
    }

    /// Drops the window in progress. The queue draining because its source
    /// went quiet is not jitter, and a window holding that drain would read
    /// it as one.
    pub fn discard_window(&mut self) {
        self.blocks = 0;
        self.sum = 0.0;
        self.low = usize::MAX;
    }

    /// Headroom to keep above one block at the queue's mean level.
    pub fn depth(&self) -> usize {
        let measured = self.measured_depth();
        if self.measured < PRIOR_WINDOWS {
            measured.max(self.prior)
        } else {
            measured
        }
    }

    fn measured_depth(&self) -> usize {
        if self.total <= 0.0 {
            return 0;
        }
        let mut above = self.total * (1.0 - QUANTILE);
        for i in (0..BUCKETS).rev() {
            above -= self.weights[i];
            if above < 0.0 {
                return edge((i + 1).min(BUCKETS - 1));
            }
        }
        edge(1)
    }

    fn record(&mut self, dip: usize, weight: f64) {
        for w in self.weights.iter_mut() {
            *w *= self.forget;
        }
        self.total *= self.forget;
        self.weights[bucket(dip)] += weight;
        self.total += weight;
    }
}

/// Watch this long after the flow resumes before ruling: a capture device
/// catches up on late callbacks within a few periods, a network within a
/// burst, and the envelope needs a few deliveries to settle.
const JUDGE_HORIZON_MS: f64 = 100.0;
/// Differences this small are rounding in the envelope, not lateness.
const JUDGE_TOLERANCE_MS: f64 = 2.0;

enum Judgement {
    Clear,
    /// The consumer is going without audio.
    Missing {
        missed: usize,
        peak_before: i64,
    },
    /// Flow is back; find where its envelope settles.
    Watching {
        missed: usize,
        peak_before: i64,
        peak_after: i64,
        watched: usize,
    },
}

/// Decides how much of a gap in delivery was late audio rather than no audio.
///
/// It keeps the running balance of frames delivered minus frames of time
/// elapsed. For a clocked source that balance saws within a fixed band, and
/// its upper envelope (the peak right after each delivery) stays put. Silence
/// removes audio for good, so after it the envelope settles lower by the
/// silence's length; late audio is still delivered, so the envelope comes back
/// to where it was. Comparing the envelopes on both sides of a gap separates
/// the two exactly, whatever size the source's deliveries are.
pub struct OutageJudge {
    state: Judgement,
    horizon: usize,
    tolerance: usize,
    balance: i64,
    peak: i64,
}

impl OutageJudge {
    pub fn new(sample_rate: u32) -> Self {
        let frames = |ms: f64| (sample_rate as f64 * ms / 1000.0).round() as usize;
        Self {
            state: Judgement::Clear,
            horizon: frames(JUDGE_HORIZON_MS),
            tolerance: frames(JUDGE_TOLERANCE_MS),
            balance: 0,
            peak: 0,
        }
    }

    /// No gap is open or being judged: the flow is steady.
    pub fn is_clear(&self) -> bool {
        matches!(self.state, Judgement::Clear)
    }

    /// The consumer came up `frames` short: a gap starts (or grows).
    pub fn missing(&mut self, frames: usize) {
        self.state = match self.state {
            Judgement::Clear => Judgement::Missing {
                missed: frames,
                peak_before: self.peak,
            },
            Judgement::Missing {
                missed,
                peak_before,
            }
            | Judgement::Watching {
                missed,
                peak_before,
                ..
            } => Judgement::Missing {
                missed: missed + frames,
                peak_before,
            },
        };
    }

    /// Once per block: `arrived` frames were delivered while `elapsed` frames
    /// of time passed. When a gap's verdict is in, returns how many of the
    /// frames the consumer went without had really been late (and were later
    /// delivered), 0 when the source had simply gone quiet.
    pub fn delivered(&mut self, arrived: usize, elapsed: usize) -> Option<usize> {
        self.balance += arrived as i64 - elapsed as i64;
        match &mut self.state {
            Judgement::Clear => {
                // The envelope follows the peaks, sinking slowly between them
                // so it can track a capture clock drifting against ours.
                self.peak = self.balance.max(self.peak - (elapsed / 64) as i64);
                None
            }
            Judgement::Missing {
                missed,
                peak_before,
            } => {
                if arrived == 0 {
                    *missed += elapsed;
                } else {
                    self.state = Judgement::Watching {
                        missed: *missed,
                        peak_before: *peak_before,
                        peak_after: self.balance,
                        watched: elapsed,
                    };
                }
                None
            }
            Judgement::Watching {
                missed,
                peak_before,
                peak_after,
                watched,
            } => {
                *peak_after = (*peak_after).max(self.balance);
                *watched += elapsed;
                if *watched < self.horizon {
                    return None;
                }
                let gone_for_good = (*peak_before - *peak_after).max(0) as usize;
                let late = missed.saturating_sub(gone_for_good);
                let verdict = if late > self.tolerance { late } else { 0 };
                self.peak = *peak_after;
                self.state = Judgement::Clear;
                Some(verdict)
            }
        }
    }
}

#[cfg(test)]
mod judge_tests {
    use super::*;

    const SR: u32 = 48_000;
    const BLOCK: usize = 64;

    /// Drives a judge with a source delivering `burst` frames every
    /// `burst` frames of time, silent in `quiet`, and holding back (then
    /// catching up) in `late`. Returns every verdict.
    fn run(burst: usize, seconds: f64, quiet: &[(f64, f64)], late: &[(f64, f64)]) -> Vec<usize> {
        run_from(burst, seconds, quiet, late, 4 * burst)
    }

    /// `queue` is what the consumer holds when the run starts.
    fn run_from(
        burst: usize,
        seconds: f64,
        quiet: &[(f64, f64)],
        late: &[(f64, f64)],
        mut queue: usize,
    ) -> Vec<usize> {
        let mut j = OutageJudge::new(SR);
        let mut verdicts = Vec::new();
        let (mut t, mut next, mut owed_back) = (0usize, 0usize, 0usize);
        let mut refilling = false;
        let end = (seconds * SR as f64) as usize;
        let secs = |f: usize| f as f64 / SR as f64;
        while t < end {
            let mut arrived = 0;
            while next <= t {
                let s = secs(next);
                if quiet.iter().any(|&(a, b)| s >= a && s < b) {
                    // Nothing exists to deliver.
                } else if late.iter().any(|&(a, b)| s >= a && s < b) {
                    owed_back += burst;
                } else {
                    arrived += burst + owed_back;
                    owed_back = 0;
                }
                next += burst;
            }
            queue += arrived;
            if let Some(v) = j.delivered(arrived, BLOCK) {
                verdicts.push(v);
            }
            // Like a real consumer, one that ran dry refills a reserve of two
            // deliveries before it plays on.
            if refilling && queue >= 2 * burst {
                refilling = false;
            }
            if !refilling {
                if queue < BLOCK {
                    j.missing(BLOCK - queue);
                    queue = 0;
                    refilling = true;
                } else {
                    queue -= BLOCK;
                }
            }
            t += BLOCK;
        }
        verdicts
    }

    #[test]
    fn a_steady_source_is_never_judged() {
        assert!(run(480, 10.0, &[], &[]).is_empty());
    }

    #[test]
    fn a_source_that_went_quiet_was_not_late() {
        let verdicts = run(480, 20.0, &[(2.0, 7.0), (10.0, 10.08), (12.0, 12.3)], &[]);
        assert_eq!(verdicts.len(), 3, "each gap judged once: {verdicts:?}");
        assert!(
            verdicts.iter().all(|&v| v == 0),
            "silence charged: {verdicts:?}"
        );
    }

    #[test]
    fn late_audio_that_catches_up_is_charged() {
        // 50 ms of deliveries held back, then released together, into a
        // consumer holding nothing in reserve: all of it went unplayed.
        let verdicts = run_from(480, 10.0, &[], &[(3.0, 3.05)], 0);
        // A consumer with no reserve also runs dry once at startup, between
        // the first two deliveries; that gap is rightly not lateness.
        assert!(
            verdicts[..verdicts.len() - 1].iter().all(|&v| v == 0),
            "{verdicts:?}"
        );
        let late = *verdicts.last().expect("the held-back delivery was judged");
        assert!(
            (1_900..=2_900).contains(&late),
            "50 ms late should charge ~2400 frames: {late}"
        );
        // With a reserve, only what the reserve failed to cover is charged.
        let covered = *run_from(480, 10.0, &[], &[(3.0, 3.05)], 1_920)
            .last()
            .expect("judged");
        assert!(covered < late && covered > 0, "{covered} vs {late}");
    }

    #[test]
    fn a_quiet_spell_ending_in_late_audio_charges_only_the_lateness() {
        // 2 s quiet, then the first 30 ms of the resumed flow arrive late.
        let verdicts = run(480, 10.0, &[(2.0, 4.0)], &[(4.0, 4.03)]);
        assert_eq!(verdicts.len(), 1, "{verdicts:?}");
        assert!(verdicts[0] < 2_400, "charged the silence: {}", verdicts[0]);
    }

    #[test]
    fn talk_spurts_with_gaps_are_never_charged() {
        // DTX-like: 300 ms of audio, 200 ms of nothing, for a minute.
        let quiet: Vec<(f64, f64)> = (0..120)
            .map(|k| (k as f64 * 0.5 + 0.3, k as f64 * 0.5 + 0.5))
            .collect();
        let verdicts = run(960, 60.0, &quiet, &[]);
        assert!(!verdicts.is_empty());
        assert!(verdicts.iter().all(|&v| v == 0), "{verdicts:?}");
    }

    #[test]
    fn judging_never_allocates() {
        let mut j = OutageJudge::new(SR);
        crate::audio::rt_guard::assert_no_alloc("judge", || {
            for i in 0..10_000 {
                if i % 100 == 0 {
                    j.missing(64);
                }
                std::hint::black_box(j.delivered(if i % 7 == 0 { 480 } else { 0 }, 64));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn est(prior: usize) -> DepthEstimator {
        DepthEstimator::new(SR, 64, 250.0, prior)
    }

    /// Feed `windows` windows of a sawtooth: `burst` frames arrive at once
    /// and drain one 64-frame block at a time, starting from `floor`.
    fn sawtooth(e: &mut DepthEstimator, floor: usize, burst: usize, windows: usize) {
        let mut level = floor + burst;
        let mut closed = 0;
        while closed < windows {
            if e.observe(level) {
                closed += 1;
            }
            level = if level >= floor + 64 {
                level - 64
            } else {
                floor + burst
            };
        }
    }

    #[test]
    fn buckets_are_monotonic_and_cover_a_second_at_high_rates() {
        for i in 1..BUCKETS {
            assert!(edge(i) > edge(i - 1));
            assert_eq!(bucket(edge(i)), i);
            assert_eq!(bucket(edge(i) - 1), i - 1);
        }
        assert!(edge(BUCKETS - 1) > 384_000);
    }

    #[test]
    fn depth_covers_the_measured_jitter() {
        let mut e = est(0);
        sawtooth(&mut e, 200, 512, 200);
        let d = e.depth();
        assert!(
            (256..=512 + 64).contains(&d),
            "sawtooth of 512 needs ~half: {d}"
        );
    }

    // The flaw of the old targets: correcting the level moved the target.
    #[test]
    fn depth_does_not_follow_the_queue_level() {
        let mut shallow = est(0);
        let mut deep = est(0);
        sawtooth(&mut shallow, 0, 512, 200);
        sawtooth(&mut deep, 5_000, 512, 200);
        assert_eq!(shallow.depth(), deep.depth());
    }

    #[test]
    fn steady_jitter_gives_a_steady_depth() {
        let mut e = est(0);
        sawtooth(&mut e, 100, 480, 100);
        let settled = e.depth();
        for _ in 0..50 {
            sawtooth(&mut e, 100, 480, 10);
            assert_eq!(e.depth(), settled, "depth wandered under unchanged jitter");
        }
    }

    #[test]
    fn an_underrun_raises_the_depth_at_once() {
        let mut e = est(0);
        sawtooth(&mut e, 100, 256, 100);
        let before = e.depth();
        e.underrun(2_000);
        assert!(e.depth() >= before + 2_000, "{} -> {}", before, e.depth());
    }

    #[test]
    fn unused_depth_is_released_over_about_a_minute() {
        let mut e = est(0);
        sawtooth(&mut e, 100, 256, 40);
        e.underrun(4_000);
        let spiked = e.depth();
        // 10 s later the spike is still honoured.
        sawtooth(&mut e, 100, 256, 40);
        assert_eq!(e.depth(), spiked, "released too fast");
        // Four minutes of calm hand it back.
        sawtooth(&mut e, 100, 256, 960);
        assert!(e.depth() < 1_000, "never released: {}", e.depth());
    }

    #[test]
    fn a_repeating_spike_keeps_its_depth() {
        let mut e = est(0);
        for _ in 0..10 {
            sawtooth(&mut e, 100, 256, 80);
            e.underrun(3_000);
        }
        let d = e.depth();
        sawtooth(&mut e, 100, 256, 80);
        assert!(
            e.depth() >= 3_000,
            "forgot a spike that recurs every 20 s: {d}"
        );
    }

    #[test]
    fn prior_holds_until_measurements_replace_it() {
        let mut e = est(2_880);
        assert!(e.depth() >= 2_880);
        sawtooth(&mut e, 100, 128, PRIOR_WINDOWS - 1);
        assert!(e.depth() >= 2_880, "prior dropped before a second of data");
        sawtooth(&mut e, 100, 128, 1);
        assert!(
            e.depth() < 500,
            "prior outlived real measurements: {}",
            e.depth()
        );
    }

    #[test]
    fn observe_and_underrun_never_allocate() {
        let mut e = est(512);
        crate::audio::rt_guard::assert_no_alloc("depth estimator", || {
            for i in 0..10_000 {
                e.observe(i % 700);
            }
            e.underrun(300);
            std::hint::black_box(e.depth());
        });
    }
}
