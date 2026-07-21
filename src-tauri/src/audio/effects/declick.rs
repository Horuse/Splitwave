use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crate::audio::graph::DeclickData;

use super::util::load_f32;
use super::{Effect, EffectControl};

/// Longest click the node repairs; also fixes the added latency. 5 ms covers
/// mouse pops and mic taps; wider bursts are dropouts, not clicks.
const MAX_MS: f32 = 5.0;
/// Consecutive quiet samples that confirm a click has ended.
const CLOSE_HOLD: usize = 3;
/// Detector high-pass corner — clicks are broadband, their edge lives up here.
const HP_CUTOFF_HZ: f32 = 1500.0;
/// Time constant of the high-frequency floor the threshold scales against.
const FLOOR_MS: f32 = 80.0;

/// Region declicker. A high-pass detector flags a transient burst whose energy
/// jumps past a level-scaled threshold; the flagged span is then filled by a
/// Catmull-Rom curve through the clean samples on either side. Output is delayed
/// by `delay` samples so a whole burst is seen before its region is committed.
pub struct DeclickEffect {
    sensitivity: Arc<AtomicU32>,
    max_width_ms: Arc<AtomicU32>,
    sample_rate: u32,
    cap: usize,
    delay: usize,
    max_w_cap: usize,
    a_lp: f32,
    a_env: f32,
    a_fast: f32,
    warmup: usize,
    n: u64,
    ch: [ChanState; 2],
}

struct ChanState {
    buf: Vec<f32>,
    lp: f32,
    hp_level: f32,
    in_click: bool,
    click_start: u64,
    below: usize,
}

impl ChanState {
    fn new(cap: usize) -> Self {
        Self {
            buf: vec![0.0; cap],
            lp: 0.0,
            hp_level: 0.0,
            in_click: false,
            click_start: 0,
            below: 0,
        }
    }
}

impl DeclickEffect {
    pub fn new(d: DeclickData, sample_rate: u32) -> (Self, EffectControl) {
        let sensitivity = Arc::new(AtomicU32::new(d.sensitivity.clamp(0.0, 1.0).to_bits()));
        let max_width_ms = Arc::new(AtomicU32::new(d.max_width_ms.clamp(0.3, MAX_MS).to_bits()));
        let control = EffectControl::Declick {
            sensitivity: sensitivity.clone(),
            max_width_ms: max_width_ms.clone(),
        };
        (Self::build(sensitivity, max_width_ms, sample_rate), control)
    }

    pub fn from_state(
        sensitivity: Arc<AtomicU32>,
        max_width_ms: Arc<AtomicU32>,
        sample_rate: u32,
    ) -> Self {
        Self::build(sensitivity, max_width_ms, sample_rate)
    }

    fn build(
        sensitivity: Arc<AtomicU32>,
        max_width_ms: Arc<AtomicU32>,
        sample_rate: u32,
    ) -> Self {
        let sr = sample_rate as f32;
        let max_w_cap = ((sr * MAX_MS * 0.001) as usize).max(2);
        let delay = max_w_cap + CLOSE_HOLD + 4;
        let cap = delay + 8;
        let a_lp = 1.0 - (-2.0 * std::f32::consts::PI * HP_CUTOFF_HZ / sr).exp();
        let a_env = 1.0 - (-1.0 / (FLOOR_MS * 0.001 * sr)).exp();
        // Settle the HF floor quickly, then detect; a slow floor would still be
        // ramping and flag ordinary signal as clicks.
        let a_fast = 1.0 - (-1.0 / (2.0 * 0.001 * sr)).exp();
        let warmup = max_w_cap.max((sr * 0.01) as usize);
        Self {
            sensitivity,
            max_width_ms,
            sample_rate,
            cap,
            delay,
            max_w_cap,
            a_lp,
            a_env,
            a_fast,
            warmup,
            n: 0,
            ch: [ChanState::new(cap), ChanState::new(cap)],
        }
    }
}

impl Effect for DeclickEffect {
    fn latency_frames(&self) -> usize {
        self.delay
    }

    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let sens = load_f32(&self.sensitivity).clamp(0.0, 1.0);
        // Higher sensitivity lowers how far above the floor a burst must sit.
        let k_on = 10.0 - 7.0 * sens;
        let k_off = k_on * 0.5;
        let max_w = (((load_f32(&self.max_width_ms).clamp(0.3, MAX_MS) * self.sample_rate as f32
            * 0.001) as usize)
            .max(2))
        .min(self.max_w_cap);

        let cap = self.cap as u64;
        for frame in samples[..frames * 2].chunks_exact_mut(2) {
            for c in 0..2 {
                let st = &mut self.ch[c];
                let x = frame[c];
                st.buf[(self.n % cap) as usize] = x;

                st.lp += (x - st.lp) * self.a_lp;
                let ah = (x - st.lp).abs();

                if self.n < self.warmup as u64 {
                    // Prime the floor fast; no detection until it is trustworthy.
                    st.hp_level += (ah - st.hp_level) * self.a_fast;
                } else if !st.in_click {
                    st.hp_level += (ah - st.hp_level) * self.a_env;
                    if ah > k_on * st.hp_level + 1e-5 {
                        st.in_click = true;
                        st.click_start = self.n;
                        st.below = 0;
                    }
                } else {
                    if ah < k_off * st.hp_level + 1e-5 {
                        st.below += 1;
                    } else {
                        st.below = 0;
                    }
                    let width = (self.n - st.click_start + 1) as usize;
                    if st.below >= CLOSE_HOLD {
                        let e = self.n - st.below as u64; // last loud sample
                        repair(&mut st.buf, self.cap, st.click_start, e);
                        st.in_click = false;
                    } else if width >= max_w {
                        // Force close, keeping two post-anchors in the buffer.
                        let e = self.n - 2;
                        if e >= st.click_start {
                            repair(&mut st.buf, self.cap, st.click_start, e);
                        }
                        st.in_click = false;
                    }
                }
            }

            for c in 0..2 {
                frame[c] = if self.n >= self.delay as u64 {
                    self.ch[c].buf[((self.n - self.delay as u64) % cap) as usize]
                } else {
                    0.0
                };
            }
            self.n += 1;
        }
    }
}

/// Fill `[s, e]` with a Catmull-Rom curve through the clean anchors at s-1/s-2
/// and e+1/e+2, all of which are already in the ring.
fn repair(buf: &mut [f32], cap: usize, s: u64, e: u64) {
    if s < 2 {
        return;
    }
    let at = |k: u64| ((k % cap as u64) as usize);
    let p0 = buf[at(s - 2)];
    let p1 = buf[at(s - 1)];
    let p2 = buf[at(e + 1)];
    let p3 = buf[at(e + 2)];
    let l = (e - s + 2) as f32; // intervals from p1 (u=0) to p2 (u=1)
    let mut i = s;
    while i <= e {
        let u = (i - s + 1) as f32 / l;
        let u2 = u * u;
        let u3 = u2 * u;
        buf[at(i)] = 0.5
            * (2.0 * p1
                + (-p0 + p2) * u
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u2
                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u3);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declick(sensitivity: f32) -> DeclickEffect {
        DeclickEffect::new(
            DeclickData {
                sensitivity,
                max_width_ms: 5.0,
                bypassed: false,
            },
            48_000,
        )
        .0
    }

    fn tone(frames: usize) -> Vec<f32> {
        let mut buf = vec![0.0f32; frames * 2];
        for f in 0..frames {
            let v = (f as f32 * 0.15).sin() * 0.3;
            buf[f * 2] = v;
            buf[f * 2 + 1] = v;
        }
        buf
    }

    #[test]
    fn wide_click_is_repaired() {
        let mut e = declick(0.5);
        let frames = 3000;
        let mut buf = tone(frames);
        // A ~25-sample burst on the left channel.
        for k in 0..25 {
            buf[(1000 + k) * 2] = if k % 2 == 0 { 0.9 } else { -0.9 };
        }
        let d = e.delay;
        e.process(&mut buf, frames);
        // Output for input frame f lands at f + delay. The repaired region must
        // no longer hold the loud burst.
        let mut peak = 0.0f32;
        for k in 0..25 {
            let out = buf[(1000 + k + d) * 2].abs();
            if out > peak {
                peak = out;
            }
        }
        assert!(peak < 0.55, "click not repaired, peak {peak}");
    }

    #[test]
    fn clean_tone_passes_through() {
        let mut e = declick(0.5);
        let frames = 3000;
        let mut buf = tone(frames);
        let reference = buf.clone();
        let d = e.delay;
        e.process(&mut buf, frames);
        // Away from priming, output frame n equals input frame n-delay.
        for n in (d + 50)..(frames - 5) {
            let got = buf[n * 2];
            let want = reference[(n - d) * 2];
            assert!((got - want).abs() < 0.02, "altered clean sample at {n}");
        }
    }
}
