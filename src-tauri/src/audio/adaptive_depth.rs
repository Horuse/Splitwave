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
//! Everything is fixed-size: `observe` and `underrun` are RT-safe.

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
