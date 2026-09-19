use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::ReverbData;

use super::util::load_f32;
use super::{Effect, EffectControl};

/// Freeverb. Tuning constants are 44.1 kHz; we scale buffer lengths to the
/// host SR. Right channel uses `STEREO_SPREAD`-sample longer buffers — the
/// length offset is what gives the wet field stereo image.
pub struct ReverbEffect {
    room_size: Arc<AtomicU32>,
    damping: Arc<AtomicU32>,
    width: Arc<AtomicU32>,
    mix: Arc<AtomicU32>,
    comb_l: [Comb; 8],
    comb_r: [Comb; 8],
    allpass_l: [Allpass; 4],
    allpass_r: [Allpass; 4],
}

struct Comb {
    buf: Box<[f32]>,
    pos: usize,
    filter_store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)].into_boxed_slice(),
            pos: 0,
            filter_store: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.pos];
        self.filter_store = out * (1.0 - damp) + self.filter_store * damp;
        self.buf[self.pos] = input + self.filter_store * feedback;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        out
    }
}

struct Allpass {
    buf: Box<[f32]>,
    pos: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)].into_boxed_slice(),
            pos: 0,
        }
    }

    /// Fixed g = 0.5 — not a true allpass, but it's what defines the
    /// Freeverb sound. Don't parametrise it.
    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buf[self.pos];
        let output = -input + buf_out;
        self.buf[self.pos] = input + buf_out * 0.5;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        output
    }
}

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const REVERB_INPUT_GAIN: f32 = 0.015;
const REVERB_SCALE_ROOM: f32 = 0.28;
const REVERB_OFFSET_ROOM: f32 = 0.7;
const REVERB_SCALE_DAMP: f32 = 0.4;

impl ReverbEffect {
    pub fn new(d: ReverbData, sample_rate: u32) -> (Self, EffectControl) {
        let room_size = Arc::new(AtomicU32::new(d.room_size.clamp(0.0, 1.0).to_bits()));
        let damping = Arc::new(AtomicU32::new(d.damping.clamp(0.0, 1.0).to_bits()));
        let width = Arc::new(AtomicU32::new(d.width.clamp(0.0, 1.0).to_bits()));
        let mix = Arc::new(AtomicU32::new(d.mix.clamp(0.0, 1.0).to_bits()));
        let control = EffectControl::Reverb {
            room_size: room_size.clone(),
            damping: damping.clone(),
            width: width.clone(),
            mix: mix.clone(),
        };
        (
            Self::from_state(room_size, damping, width, mix, sample_rate),
            control,
        )
    }

    pub fn from_state(
        room_size: Arc<AtomicU32>,
        damping: Arc<AtomicU32>,
        width: Arc<AtomicU32>,
        mix: Arc<AtomicU32>,
        sample_rate: u32,
    ) -> Self {
        let scale = sample_rate as f32 / 44100.0;
        let comb_len = |n: usize| (n as f32 * scale) as usize;
        Self {
            room_size,
            damping,
            width,
            mix,
            comb_l: std::array::from_fn(|i| Comb::new(comb_len(COMB_TUNING[i]))),
            comb_r: std::array::from_fn(|i| Comb::new(comb_len(COMB_TUNING[i] + STEREO_SPREAD))),
            allpass_l: std::array::from_fn(|i| Allpass::new(comb_len(ALLPASS_TUNING[i]))),
            allpass_r: std::array::from_fn(|i| {
                Allpass::new(comb_len(ALLPASS_TUNING[i] + STEREO_SPREAD))
            }),
        }
    }
}

impl Effect for ReverbEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        let room = load_f32(&self.room_size).clamp(0.0, 1.0);
        let damping = load_f32(&self.damping).clamp(0.0, 1.0);
        let width = load_f32(&self.width).clamp(0.0, 1.0);
        let mix = load_f32(&self.mix).clamp(0.0, 1.0);
        let feedback = room * REVERB_SCALE_ROOM + REVERB_OFFSET_ROOM;
        let damp = damping * REVERB_SCALE_DAMP;
        let dry = 1.0 - mix;
        // width=1: strict L/R wet; width=0: mono wet image (cross-channel mix)
        let wet1 = mix * (width * 0.5 + 0.5);
        let wet2 = mix * (1.0 - width) * 0.5;

        let stereo = &mut samples[..frames * 2];
        for frame in stereo.chunks_exact_mut(2) {
            let il = frame[0];
            let ir = frame[1];
            let input = (il + ir) * REVERB_INPUT_GAIN;
            let mut out_l = 0.0;
            let mut out_r = 0.0;
            for c in &mut self.comb_l {
                out_l += c.process(input, feedback, damp);
            }
            for c in &mut self.comb_r {
                out_r += c.process(input, feedback, damp);
            }
            for ap in &mut self.allpass_l {
                out_l = ap.process(out_l);
            }
            for ap in &mut self.allpass_r {
                out_r = ap.process(out_r);
            }
            frame[0] = il * dry + out_l * wet1 + out_r * wet2;
            frame[1] = ir * dry + out_r * wet1 + out_l * wet2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const SR: u32 = 48_000;

    fn fresh(mix: f32, width: f32) -> ReverbEffect {
        let d = ReverbData {
            room_size: 0.8,
            damping: 0.5,
            width,
            mix,
            bypassed: false,
        };
        let (e, _) = ReverbEffect::new(d, SR);
        e
    }

    fn impulse(frames: usize) -> Vec<f32> {
        let mut buf = vec![0.0; frames * 2];
        buf[0] = 1.0;
        buf[1] = 1.0;
        buf
    }

    #[test]
    fn silence_stays_silent() {
        let mut e = fresh(0.5, 1.0);
        let mut buf = vec![0.0; 240 * 2];
        e.process(&mut buf, 240);
        assert!(buf.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn zero_mix_is_bit_exact_passthrough() {
        let mut e = fresh(0.0, 1.0);
        let input = vec![0.3, -0.7, 0.9, -0.1, 0.4, 0.4, -0.9, 0.2];
        let mut buf = input.clone();
        e.process(&mut buf, 4);
        assert_eq!(buf, input);
    }

    #[test]
    fn full_mix_removes_dry() {
        let mut e = fresh(1.0, 1.0);
        let mut buf = vec![0.5; 96 * 2];
        e.process(&mut buf, 96);
        // dry = 0: the output must be reverb tail only, never the input echo.
        // Combs need ~1617+ frames at 44.1k to fill; early samples stay near 0.
        assert!(buf[..32].iter().all(|s| s.abs() < 0.1));
    }

    #[test]
    fn tail_decays_and_stays_stable() {
        let mut e = fresh(1.0, 1.0);
        let mut buf = vec![1.0; 96 * 2];
        e.process(&mut buf, 96);
        // Max feedback = 1.0*0.28 + 0.7 = 0.98 < 1: stable, no explosion.
        let mut energies = Vec::new();
        for _ in 0..50 {
            let mut zero = vec![0.0; 9600 * 2];
            e.process(&mut zero, 4800);
            assert!(zero.iter().all(|s| s.is_finite()));
            assert!(
                zero.iter().all(|s| s.abs() < 100.0),
                "no runaway: {:#}",
                zero.iter().fold(0.0f32, |m, s| m.max(s.abs()))
            );
            energies.push(zero.iter().map(|s| s * s).sum::<f32>());
        }
        assert!(energies[49] < energies[0] * 1e-3);
    }

    #[test]
    fn impulse_response_decays() {
        let mut e = fresh(1.0, 1.0);
        // Impulse, then 6 s of silence: room 0.8 → feedback 0.924 ≈ 2.5 s
        // RT60, so compare the second vs sixth second of the tail.
        let mut first = impulse(4800);
        e.process(&mut first, 2400);
        let e_first: f32 = first.iter().map(|s| s * s).sum();
        let mut tail = vec![0.0; 14400 * 2];
        e.process(&mut tail, 14400);
        let early_tail: f32 = tail[4800 * 2..9600 * 2].iter().map(|s| s * s).sum();
        let late_tail: f32 = tail[24000..].iter().map(|s| s * s).sum();
        assert!(e_first > 0.0, "impulse must reach the wet path");
        assert!(
            late_tail < early_tail * 0.9,
            "impulse response must decay: early {early_tail} late {late_tail}"
        );
    }

    #[test]
    fn full_width_gives_stereo_image() {
        let mut e = fresh(1.0, 1.0); // width=1: strict L/R wet
        let mut buf = vec![0.0; 4800 * 2];
        buf[0] = 1.0;
        buf[1] = 1.0; // mono impulse
        e.process(&mut buf, 2400);
        // R channel comb lengths are offset by STEREO_SPREAD → different tail.
        let l_sum: f32 = buf[..].iter().step_by(2).map(|s| s.abs()).sum();
        let r_sum: f32 = buf[1..].iter().step_by(2).map(|s| s.abs()).sum();
        assert!(
            (l_sum - r_sum).abs() > 0.01,
            "stereo image must differ across channels: {l_sum} vs {r_sum}"
        );
    }

    #[test]
    fn zero_width_mixes_wet_mono() {
        // width=0: wet1 = mix*0.5, wet2 = mix*0.5 → both channels share the
        // same wet content weights → symmetric output for mono input.
        let mut e = fresh(1.0, 0.0);
        let mut buf = vec![0.0; 4800 * 2];
        buf[0] = 1.0;
        buf[1] = 1.0;
        e.process(&mut buf, 2400);
        for (l, r) in buf.chunks_exact(2).map(|f| (f[0], f[1])) {
            assert!(
                (l - r).abs() < 1e-4,
                "zero width must be mono wet: {l} vs {r}"
            );
        }
    }

    #[test]
    fn new_clamps_extreme_params() {
        let d = ReverbData {
            room_size: 5.0,
            damping: -1.0,
            width: 100.0,
            mix: 2.0,
            bypassed: false,
        };
        let (mut e, c) = ReverbEffect::new(d, SR);
        let EffectControl::Reverb {
            room_size,
            damping,
            width,
            mix,
        } = &c
        else {
            panic!("wrong control variant");
        };
        assert_eq!(load_f32(room_size), 1.0);
        assert_eq!(load_f32(damping), 0.0);
        assert_eq!(load_f32(width), 1.0);
        assert_eq!(load_f32(mix), 1.0);
        let mut buf = vec![0.7; 96 * 2];
        e.process(&mut buf, 48);
        assert!(buf.iter().all(|s| s.is_finite()));
        assert!(buf.iter().all(|s| s.abs() < 10.0));
    }

    proptest! {
        #[test]
        fn output_is_finite_and_bounded(
            seed in 0u64..100_000,
            room in -1.0f32..2.0,
            damp in -1.0f32..2.0,
            width in -1.0f32..2.0,
            mix in -1.0f32..2.0,
        ) {
            let d = ReverbData {
                room_size: room,
                damping: damp,
                width,
                mix,
                bypassed: false,
            };
            let (mut e, _) = ReverbEffect::new(d, SR);
            // Deterministic LCG noise block.
            let mut x = seed | 1;
            let mut buf = vec![0.0f32; 256 * 2];
            for i in 0..256 * 2 {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                buf[i] = ((x >> 33) as u32 as f32 / u32::MAX as f32) * 2.0 - 1.0;
            }
            e.process(&mut buf, 256);
            for s in &buf {
                prop_assert!(s.is_finite());
                prop_assert!(s.abs() < 50.0, "runaway reverb: {s}");
            }
        }
    }
    #[test]
    fn from_state_rebuild_processes() {
        let room = Arc::new(AtomicU32::new(0.8f32.to_bits()));
        let damping = Arc::new(AtomicU32::new(0.5f32.to_bits()));
        let width = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let mix = Arc::new(AtomicU32::new(0.5f32.to_bits()));
        let mut e = ReverbEffect::from_state(
            room.clone(),
            damping.clone(),
            width.clone(),
            mix.clone(),
            SR,
        );
        mix.store(0.0f32.to_bits(), std::sync::atomic::Ordering::Relaxed);
        let input = vec![0.3, -0.7, 0.9, -0.1];
        let mut buf = input.clone();
        e.process(&mut buf, 2);
        assert_eq!(buf, input);
    }
}
