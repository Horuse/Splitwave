use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::LimiterData;

use super::util::{db_to_linear, load_f32, store_f32};
use super::{Effect, EffectControl};

/// Brick-wall limiter: input is delayed by `lookahead_frames`. The gain each
/// frame needs is held over the lookahead window and averaged over it again,
/// so the gain falls in a smooth ramp that reaches what a peak needs exactly
/// as the peak emerges, never a step. Exponential release.
pub struct LimiterEffect {
    ceiling: Arc<AtomicU32>,
    release_ms: Arc<AtomicU32>,
    sample_rate: u32,
    lookahead_frames: usize,
    /// Stereo-interleaved look-ahead delay; both channels share `current_gain`.
    delay_buf: Box<[f32]>,
    delay_pos: usize,
    /// Peak of the last `window` frames, the one emerging now included.
    peaks: SlidingMax,
    /// The held gain of the last `window` frames, and their sum.
    held: Box<[f32]>,
    held_pos: usize,
    held_sum: f64,
    current_gain: f32,
    /// Min gain during the last block (1.0 = no limiting).
    pub gr_lin: Arc<AtomicU32>,
}

/// Largest of the last `capacity` values pushed, in constant time per push:
/// a queue of values each larger than all pushed after it.
struct SlidingMax {
    values: Box<[f32]>,
    times: Box<[u64]>,
    head: usize,
    len: usize,
    now: u64,
}

impl SlidingMax {
    fn new(capacity: usize) -> Self {
        Self {
            values: vec![0.0; capacity].into_boxed_slice(),
            times: vec![0; capacity].into_boxed_slice(),
            head: 0,
            len: 0,
            now: 0,
        }
    }

    #[inline]
    fn push(&mut self, v: f32) -> f32 {
        let cap = self.values.len();
        if self.len > 0 && self.now >= cap as u64 && self.times[self.head] <= self.now - cap as u64
        {
            self.head = (self.head + 1) % cap;
            self.len -= 1;
        }
        let at = |i: usize| (self.head + i) % cap;
        while self.len > 0 && self.values[at(self.len - 1)] <= v {
            self.len -= 1;
        }
        let tail = at(self.len);
        self.values[tail] = v;
        self.times[tail] = self.now;
        self.len += 1;
        self.now += 1;
        self.values[self.head]
    }
}

impl LimiterEffect {
    pub fn new(d: LimiterData, sample_rate: u32) -> (Self, EffectControl, Arc<AtomicU32>) {
        let lookahead_frames =
            ((d.lookahead_ms.max(0.1) * sample_rate as f32 / 1000.0) as usize).max(1);
        let ceiling_lin = db_to_linear(d.ceiling_db).max(1e-6);
        let ceiling = Arc::new(AtomicU32::new(ceiling_lin.to_bits()));
        let release_ms = Arc::new(AtomicU32::new(d.release_ms.max(0.1).to_bits()));
        let gr_lin = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let control = EffectControl::Limiter {
            ceiling: ceiling.clone(),
            release_ms: release_ms.clone(),
        };
        (
            Self::from_state(
                ceiling,
                release_ms,
                lookahead_frames,
                sample_rate,
                gr_lin.clone(),
            ),
            control,
            gr_lin,
        )
    }

    pub fn from_state(
        ceiling: Arc<AtomicU32>,
        release_ms: Arc<AtomicU32>,
        lookahead_frames: usize,
        sample_rate: u32,
        gr_lin: Arc<AtomicU32>,
    ) -> Self {
        // The frame emerging now and every frame still in the delay.
        let window = lookahead_frames + 1;
        Self {
            ceiling,
            release_ms,
            sample_rate,
            lookahead_frames,
            delay_buf: vec![0.0; lookahead_frames * 2].into_boxed_slice(),
            delay_pos: 0,
            peaks: SlidingMax::new(window),
            held: vec![1.0; window].into_boxed_slice(),
            held_pos: 0,
            held_sum: window as f64,
            current_gain: 1.0,
            gr_lin,
        }
    }
}

impl Effect for LimiterEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let ceiling = load_f32(&self.ceiling).max(1e-6);
        let release_ms = load_f32(&self.release_ms).max(0.1);
        let release_coeff = 1.0 - (-1.0 / (release_ms * 0.001 * self.sample_rate as f32)).exp();

        let lookahead = self.lookahead_frames;
        let window = self.held.len();
        let stereo = &mut samples[..frames * 2];
        let mut block_min_gr = 1.0f32;
        for f in 0..frames {
            let l_in = stereo[f * 2];
            let r_in = stereo[f * 2 + 1];

            // Read the emerging (oldest) sample, then overwrite that slot.
            let l_out = self.delay_buf[self.delay_pos * 2];
            let r_out = self.delay_buf[self.delay_pos * 2 + 1];
            self.delay_buf[self.delay_pos * 2] = l_in;
            self.delay_buf[self.delay_pos * 2 + 1] = r_in;
            self.delay_pos = if self.delay_pos + 1 == lookahead {
                0
            } else {
                self.delay_pos + 1
            };

            // The gain the loudest frame in the window needs, held over the
            // window, then averaged over it: every frame averaged in saw the
            // emerging one, so the average never asks for more than it needs.
            let peak = self.peaks.push(l_in.abs().max(r_in.abs()));
            let need = if peak > ceiling { ceiling / peak } else { 1.0 };
            self.held_sum += need as f64 - self.held[self.held_pos] as f64;
            self.held[self.held_pos] = need;
            self.held_pos = if self.held_pos + 1 == window {
                0
            } else {
                self.held_pos + 1
            };
            let smooth = ((self.held_sum / window as f64) as f32).min(1.0);
            if smooth < self.current_gain {
                self.current_gain = smooth;
            } else {
                self.current_gain += (smooth - self.current_gain) * release_coeff;
            }

            if self.current_gain < block_min_gr {
                block_min_gr = self.current_gain;
            }
            stereo[f * 2] = l_out * self.current_gain;
            stereo[f * 2 + 1] = r_out * self.current_gain;
        }
        store_f32(&self.gr_lin, block_min_gr);
    }

    fn latency_frames(&self) -> usize {
        self.lookahead_frames
    }
}

#[cfg(test)]
mod tests {
    use super::super::util::load_f32;
    use super::*;
    use proptest::prelude::*;

    const SR: u32 = 48_000;

    fn dc(frames: usize, amp: f32) -> Vec<f32> {
        vec![amp; frames * 2]
    }

    fn sine(frames: usize, amp: f32, freq: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR as f32).sin();
                [s, s]
            })
            .collect()
    }

    #[test]
    fn sliding_max_matches_brute_force_on_a_falling_signal() {
        let cap = 49;
        let mut m = SlidingMax::new(cap);
        let input: Vec<f32> = (0..2000).map(|i| (i as f32 * 0.003).sin().abs()).collect();
        for (i, &v) in input.iter().enumerate() {
            let want = input[i.saturating_sub(cap - 1)..=i]
                .iter()
                .copied()
                .fold(f32::MIN, f32::max);
            assert_eq!(m.push(v), want, "frame {i}");
            assert!(m.len <= cap);
        }
    }

    #[test]
    fn below_ceiling_is_exact_delayed_passthrough() {
        let d = LimiterData {
            ceiling_db: -6.0,
            lookahead_ms: 2.0, // 96 frames @ 48k
            release_ms: 50.0,
            bypassed: false,
        };
        let (mut e, _, _) = LimiterEffect::new(d, SR);
        let input = sine(240, 0.1, 1000.0);
        let mut buf = input.clone();
        e.process(&mut buf, 240);
        // First `lookahead` frames are the zero-initialised delay line.
        assert!(buf[..96 * 2].iter().all(|s| *s == 0.0));
        // Rest is the input, bit-exact, shifted by the lookahead.
        for i in 0..(240 - 96) * 2 {
            assert_eq!(buf[96 * 2 + i], input[i], "sample {i}");
        }
    }

    #[test]
    fn brickwall_never_exceeds_ceiling_after_lookahead() {
        let d = LimiterData {
            ceiling_db: -3.0,
            lookahead_ms: 1.0,
            release_ms: 50.0,
            bypassed: false,
        };
        let (mut e, _, _) = LimiterEffect::new(d, SR);
        let input = dc(1200, 1.0); // 6 dB over a -3 dBFS ceiling
        let mut buf = input.clone();
        e.process(&mut buf, 1200);
        let ceiling = db_to_linear(-3.0);
        for (i, s) in buf.iter().enumerate().skip(48 * 2) {
            assert!(s.abs() <= ceiling + 1e-5, "sample {i} exceeds ceiling: {s}");
        }
    }

    #[test]
    fn the_gain_ramps_down_ahead_of_a_peak_instead_of_stepping() {
        // A quiet passage, then a burst 6 dB over the ceiling: the quiet
        // audio still emerging must fade, not drop at once when the burst
        // enters the lookahead.
        let d = LimiterData {
            ceiling_db: -6.0,
            lookahead_ms: 5.0,
            release_ms: 50.0,
            bypassed: false,
        };
        let (mut e, _, _) = LimiterEffect::new(d, SR);
        let mut buf = [dc(2_000, 0.25), dc(2_000, 1.0)].concat();
        e.process(&mut buf, 4_000);
        let ceiling = db_to_linear(-6.0);
        let left: Vec<f32> = buf.iter().step_by(2).copied().collect();
        // From the first emerging frame to the last of the quiet passage.
        let worst = left[240..2_000 + 240]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // The ramp halves 0.25 over 241 frames: about 0.0005 a frame.
        assert!(worst < 0.002, "the quiet audio stepped by {worst}");
        assert!(left.iter().all(|s| s.abs() <= ceiling + 1e-5));
    }

    #[test]
    fn latency_equals_lookahead_frames() {
        let d = LimiterData {
            ceiling_db: -1.0,
            lookahead_ms: 3.0,
            release_ms: 50.0,
            bypassed: false,
        };
        let (e, _, _) = LimiterEffect::new(d, SR);
        assert_eq!(e.latency_frames(), 144);
        assert_eq!((3.0 * SR as f32 / 1000.0) as usize, 144);
    }

    #[test]
    fn gain_reduction_is_reported_when_limiting() {
        let d = LimiterData {
            ceiling_db: -12.0,
            lookahead_ms: 1.0,
            release_ms: 50.0,
            bypassed: false,
        };
        let (mut e, _, gr) = LimiterEffect::new(d, SR);
        let mut buf = sine(480, 1.0, 1000.0);
        e.process(&mut buf, 240);
        let gr_lin = load_f32(&gr);
        assert!(gr_lin < 1.0, "GR must report reduction, got {gr_lin}");
        // And it should be roughly ceiling / peak.
        let expected = db_to_linear(-12.0);
        assert!(
            (gr_lin - expected).abs() < 0.05 * expected,
            "gr {gr_lin} vs {expected}"
        );
    }

    #[test]
    fn release_recovers_after_loud_passage() {
        let d = LimiterData {
            ceiling_db: -3.0,
            lookahead_ms: 1.0,
            release_ms: 20.0,
            bypassed: false,
        };
        let (mut e, _, _) = LimiterEffect::new(d, SR);
        let mut loud = dc(240, 1.0); // 6 dB over the ceiling
        e.process(&mut loud, 240);
        // Loud burst gone: after the release tail the limiter must pass
        // quiet input through unattenuated.
        let quiet = dc(240, 0.05);
        let mut last = vec![0.0; 240 * 2];
        for _ in 0..30 {
            let mut b = quiet.clone();
            e.process(&mut b, 240);
            last.copy_from_slice(&b);
        }
        // Release τ = 20 ms ≈ 960 frames; 30 blocks leave ≈ e^-6 residual.
        let peak = last.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 0.048,
            "gain must recover to ~1.0 after release, peak {peak}"
        );
        assert!(peak < 0.05 * 1.01, "gain must not overshoot 1.0: {peak}");
    }

    #[test]
    fn new_clamps_hostile_params() {
        let d = LimiterData {
            ceiling_db: -200.0,
            lookahead_ms: 0.0,
            release_ms: 0.0,
            bypassed: false,
        };
        let (mut e, c, _) = LimiterEffect::new(d, SR);
        let EffectControl::Limiter {
            ceiling,
            release_ms,
        } = &c
        else {
            panic!("wrong control variant");
        };
        assert_eq!(load_f32(ceiling), db_to_linear(-60.0).max(1e-6));
        assert_eq!(load_f32(release_ms), 0.1);
        let mut buf = vec![0.5; 96];
        e.process(&mut buf, 48);
        assert!(buf.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn control_clamps_ceiling_and_release() {
        let d = LimiterData {
            ceiling_db: -6.0,
            lookahead_ms: 1.0,
            release_ms: 50.0,
            bypassed: false,
        };
        let (_, c, _) = LimiterEffect::new(d, SR);
        let EffectControl::Limiter {
            ceiling,
            release_ms,
        } = &c
        else {
            panic!("wrong control variant");
        };
        let mut update = serde_json::Map::new();
        update.insert("ceilingDb".into(), serde_json::json!(-120.0));
        update.insert("releaseMs".into(), serde_json::json!(0.0));
        c.apply_update(&serde_json::Value::Object(update));
        assert_eq!(load_f32(ceiling.as_ref()), 1e-6);
        assert_eq!(load_f32(release_ms.as_ref()), 0.1);
    }

    proptest::proptest! {
        #[test]
        fn limiter_output_is_finite_and_bounded(
            input in proptest::prelude::prop::collection::vec(-2.0f32..2.0, 512),
            ceiling_db in -30.0f32..6.0,
        ) {
            let d = LimiterData {
                ceiling_db,
                lookahead_ms: 1.0,
                release_ms: 50.0,
                bypassed: false,
            };
            let (mut e, _, _) = LimiterEffect::new(d, SR);
            let mut buf = input.clone();
            e.process(&mut buf, 256);
            let ceiling = db_to_linear(ceiling_db).max(1e-6);
            for s in buf.iter().skip(48 * 2) {
                prop_assert!(s.is_finite());
                prop_assert!(s.abs() <= ceiling + 1e-4, "|{s}| > {ceiling}");
            }
            let _ = input;
        }
    }
}
