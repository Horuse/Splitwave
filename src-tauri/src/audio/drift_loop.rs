//! Clock-drift correction for a queue that crosses clock domains, the way
//! zita-a2j/j2a do it: the consumer reads through an asynchronous resampler
//! whose ratio a slow second-order loop steers, so the queue's mean settles on
//! its target and the two clocks' drift is absorbed smoothly, never spliced.
//!
//! The loop must see the queue as a smooth function of time. Sampled only at
//! the consumer's reads, the level saws with every delivery and, worse, its
//! sampled mean steps by a whole block each time the two clocks' phases slide
//! past each other: a step no lowpass removes, which the loop would chase with
//! hundreds of ppm. So the caller adds what the producer has made but not yet
//! delivered (its rate times the time since its last write, see
//! `input_bridge::WriteClock`), which cancels both. A two-pole lowpass at
//! twenty times the loop bandwidth takes out the jitter that is left.
//!
//! Plant: the error `x` (frames above target) moves as `dx/dt = d - r*u`,
//! where `d` is the drift in frames per second, `r` the consumer rate and `u`
//! the fractional extra consumption. With `u = (kp*x + I)/r`, `I' = ki*x`, the
//! loop is `s^2 + kp*s + ki = 0`; `kp = sqrt(2)*w`, `ki = w^2` is critically
//! damped at `w = 2*pi*B`.

/// Steady-state loop bandwidth: slow enough that ratio modulation stays far
/// below anything audible as pitch movement.
const BANDWIDTH_HZ: f64 = 0.05;
/// Bandwidth while settling after a start, so the initial offset clears in
/// seconds rather than a minute.
const SETTLE_BANDWIDTH_HZ: f64 = 0.5;
const SETTLE_SECONDS: f64 = 4.0;
/// The error lowpass sits this far above the loop bandwidth.
const LOWPASS_FACTOR: f64 = 20.0;
/// Largest correction: 1000 ppm, about 1.7 cents, beyond any real crystal.
const MAX_CORRECTION: f64 = 1.0e-3;

/// How fast the producer's timing estimate follows its write times: over
/// thousands of deliveries, so single late ones average out. The lag it leaves
/// under drift is a constant offset the loop's integrator absorbs.
const ARRIVAL_SMOOTHING_S: f64 = 4.0;

/// The producer's delivery timing, smoothed: the time its frame `n` was due,
/// as `n / rate + offset`, with `offset` lowpassed across writes. From it the
/// consumer knows how many frames the producer has made but not yet written
/// without the jitter of any single delivery.
pub struct ArrivalClock {
    rate: f64,
    offset: f64,
    last_frames: u64,
    last_at: f64,
    primed: bool,
}

impl ArrivalClock {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            rate: sample_rate.max(1) as f64,
            offset: 0.0,
            last_frames: 0,
            last_at: 0.0,
            primed: false,
        }
    }

    /// Forget the timing: after a gap the producer resumes on a new offset.
    pub fn reset(&mut self) {
        self.primed = false;
    }

    /// The producer has written `frames` in total, the last of them at `at`
    /// seconds. Cheap to call every block; only a new write moves anything.
    pub fn observe(&mut self, frames: u64, at: f64) {
        if self.primed && frames == self.last_frames {
            return;
        }
        let raw = at - frames as f64 / self.rate;
        if !self.primed {
            self.offset = raw;
            self.primed = true;
        } else {
            let dt = (at - self.last_at).max(0.0);
            let a = 1.0 - (-dt / ARRIVAL_SMOOTHING_S).exp();
            self.offset += a * (raw - self.offset);
        }
        self.last_frames = frames;
        self.last_at = at;
    }

    /// Frames the producer has made by `now` but not yet written, against its
    /// smoothed schedule. Negative when a delivery landed ahead of that
    /// schedule, as half of them do; clamping those to zero would put the
    /// delivery jitter straight back. Capped at `cap` so a stalled producer
    /// does not read as an ever-fuller queue.
    pub fn pending(&self, now: f64, cap: f64) -> f64 {
        if !self.primed {
            return 0.0;
        }
        ((now - self.offset) * self.rate - self.last_frames as f64).min(cap)
    }
}

pub struct DriftLoop {
    rate: f64,
    dt: f64,
    elapsed: f64,
    lp1: f64,
    lp2: f64,
    integral: f64,
    correction: f64,
    primed: bool,
}

impl DriftLoop {
    /// `sample_rate` and `block_frames` are the consumer's: `update` is
    /// called once per block.
    pub fn new(sample_rate: u32, block_frames: usize) -> Self {
        Self {
            rate: sample_rate.max(1) as f64,
            dt: block_frames.max(1) as f64 / sample_rate.max(1) as f64,
            elapsed: 0.0,
            lp1: 0.0,
            lp2: 0.0,
            integral: 0.0,
            correction: 0.0,
            primed: false,
        }
    }

    /// Restart the settling phase and forget the filtered error, keeping the
    /// drift the integrator has learned: after a gap the queue is realigned,
    /// but the two clocks drift exactly as before.
    pub fn restart(&mut self) {
        self.elapsed = 0.0;
        self.primed = false;
    }

    /// Once per block with the queue's error against its target, in frames
    /// (positive: too much queued). Returns the fractional extra consumption:
    /// read `1 + u` input frames per output frame.
    pub fn update(&mut self, error_frames: f64) -> f64 {
        if !self.primed {
            // Start the filter at the current error so the first blocks do
            // not read as a step.
            self.lp1 = error_frames;
            self.lp2 = error_frames;
            self.primed = true;
        }
        let settling = self.elapsed < SETTLE_SECONDS;
        let bw = if settling {
            SETTLE_BANDWIDTH_HZ
        } else {
            BANDWIDTH_HZ
        };
        self.elapsed += self.dt;
        let w = std::f64::consts::TAU * bw;
        let a = 1.0 - (-std::f64::consts::TAU * bw * LOWPASS_FACTOR * self.dt).exp();
        self.lp1 += a * (error_frames - self.lp1);
        self.lp2 += a * (self.lp1 - self.lp2);
        let x = self.lp2;
        let kp = std::f64::consts::SQRT_2 * w;
        let ki = w * w;
        let proposed = (kp * x + self.integral + ki * x * self.dt) / self.rate;
        // Anti-windup: stop integrating while the output is pinned.
        if proposed.abs() < MAX_CORRECTION {
            self.integral += ki * x * self.dt;
        }
        self.correction =
            ((kp * x + self.integral) / self.rate).clamp(-MAX_CORRECTION, MAX_CORRECTION);
        self.correction
    }

    #[cfg(test)]
    pub fn correction(&self) -> f64 {
        self.correction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    struct Run {
        /// Largest |error| over the last `tail` seconds, frames.
        tail_error: f64,
        /// Mean correction over the tail, and its peak-to-peak.
        tail_u: f64,
        tail_u_ripple: f64,
        min_queue: f64,
    }

    /// A producer delivering `burst` frames at a clock `ppm` off ours into a
    /// queue that a consumer reads `block` frames at a time through a
    /// resampler steered by the loop. The loop is fed the queue plus what the
    /// producer has made since its last delivery, as the pipeline feeds it.
    /// The queue starts `offset` frames off target.
    fn run(ppm: f64, burst: usize, block: usize, offset: f64, seconds: f64, tail: f64) -> Run {
        run_jittered(ppm, burst, block, offset, seconds, tail, 0.0)
    }

    /// As `run`, each delivery landing up to `jitter` seconds late.
    fn run_jittered(
        ppm: f64,
        burst: usize,
        block: usize,
        offset: f64,
        seconds: f64,
        tail: f64,
        jitter: f64,
    ) -> Run {
        let mut rng: u32 = 0x1234_5678;
        let mut l = DriftLoop::new(SR, block);
        let target = 2_000.0;
        let mut queue = target + offset;
        let in_rate = SR as f64 * (1.0 + ppm * 1e-6);
        let in_period = burst as f64 / in_rate;
        let out_period = block as f64 / SR as f64;
        let (mut t, mut next_in, mut written) = (0.0, in_period, 0u64);
        let mut last_late = 0.0;
        let mut clock = ArrivalClock::new(SR);
        let mut r = Run {
            tail_error: 0.0,
            tail_u: 0.0,
            tail_u_ripple: 0.0,
            min_queue: f64::MAX,
        };
        let (mut umin, mut umax, mut usum, mut n) = (f64::MAX, f64::MIN, 0.0, 0usize);
        while t < seconds {
            while next_in <= t {
                queue += burst as f64;
                written += burst as u64;
                clock.observe(written, next_in);
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let late = jitter * (rng % 1000) as f64 / 1000.0;
                // Late by `late` against the producer's own schedule.
                next_in = (next_in - last_late) + in_period + late;
                last_late = late;
            }
            let accrued = clock.pending(t, 4.0 * burst as f64);
            let u = l.update(queue + accrued - target);
            queue -= block as f64 * (1.0 + u);
            r.min_queue = r.min_queue.min(queue);
            if t > seconds - tail {
                r.tail_error = r.tail_error.max((l.lp2).abs());
                umin = umin.min(u);
                umax = umax.max(u);
                usum += u;
                n += 1;
            }
            t += out_period;
        }
        r.tail_u = usum / n.max(1) as f64;
        r.tail_u_ripple = umax - umin;
        r
    }

    #[test]
    fn drift_is_absorbed_without_a_standing_error() {
        for ppm in [-500.0, -100.0, -20.0, 0.0, 20.0, 100.0, 500.0] {
            let r = run(ppm, 512, 64, 0.0, 240.0, 20.0);
            // A faster producer is consumed faster: u settles on the drift.
            assert!(
                (r.tail_u - ppm * 1e-6).abs() < 2e-6,
                "{ppm} ppm: settled at {:.1} ppm",
                r.tail_u * 1e6
            );
            assert!(
                r.tail_error < 8.0,
                "{ppm} ppm: error {:.1} frames",
                r.tail_error
            );
        }
    }

    #[test]
    fn the_delivery_saw_does_not_modulate_the_ratio() {
        // 512-frame deliveries saw the queue by 512 frames at ~94 Hz; after the
        // lowpass the ratio must hold within a fraction of a ppm.
        let r = run(50.0, 512, 64, 0.0, 240.0, 20.0);
        assert!(
            r.tail_u_ripple < 0.5e-6,
            "ripple {:.3} ppm",
            r.tail_u_ripple * 1e6
        );
    }

    #[test]
    fn delivery_jitter_does_not_modulate_the_ratio() {
        // A normalizer thread polling every 1 ms: each delivery up to 1 ms late.
        // 10 ppm peak to peak is under 0.02 cents, far below hearing, and it
        // moves no faster than the error lowpass lets it.
        let r = run_jittered(50.0, 256, 64, 0.0, 240.0, 20.0, 0.001);
        assert!(
            r.tail_u_ripple < 10.0e-6,
            "ripple {:.3} ppm",
            r.tail_u_ripple * 1e6
        );
        assert!(
            (r.tail_u - 50e-6).abs() < 3e-6,
            "settled at {:.1} ppm",
            r.tail_u * 1e6
        );
    }

    #[test]
    fn an_initial_offset_clears_while_settling() {
        let r = run(0.0, 256, 64, 800.0, 30.0, 5.0);
        assert!(
            r.tail_error < 40.0,
            "error {:.1} frames after 25 s",
            r.tail_error
        );
    }

    #[test]
    fn corrections_are_bounded() {
        let mut l = DriftLoop::new(SR, 64);
        for _ in 0..100_000 {
            let u = l.update(1_000_000.0);
            assert!(u.abs() <= MAX_CORRECTION + 1e-12);
        }
        // Anti-windup: once the error clears, it lets go promptly.
        for _ in 0..(SR as usize / 64) * 30 {
            l.update(0.0);
        }
        assert!(l.correction().abs() < MAX_CORRECTION, "wound up");
    }

    #[test]
    fn restart_keeps_the_learned_drift() {
        let mut l = DriftLoop::new(SR, 64);
        let mut queue = 2_000.0;
        // Learn 200 ppm.
        for _ in 0..(SR as usize / 64) * 120 {
            queue += 64.0 * 1.0002;
            let u = l.update(queue - 2_000.0);
            queue -= 64.0 * (1.0 + u);
        }
        let learned = l.correction();
        l.restart();
        let after = l.update(0.0);
        assert!((after - learned).abs() < 20e-6, "{learned} -> {after}");
    }

    #[test]
    fn updating_never_allocates() {
        let mut l = DriftLoop::new(SR, 64);
        crate::audio::rt_guard::assert_no_alloc("drift loop", || {
            for i in 0..10_000 {
                std::hint::black_box(l.update((i % 700) as f64));
            }
        });
    }
}
