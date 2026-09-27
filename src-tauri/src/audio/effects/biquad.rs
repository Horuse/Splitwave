/// RBJ cookbook biquad in Transposed Direct Form II — one state pair (z1, z2)
/// per channel, half the rounding noise of DF I.
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    /// Copy another biquad's coefficients while keeping this one's state — lets a
    /// filter be retuned live without a discontinuity.
    #[inline]
    pub(super) fn retune(&mut self, c: Biquad) {
        self.b0 = c.b0;
        self.b1 = c.b1;
        self.b2 = c.b2;
        self.a1 = c.a1;
        self.a2 = c.a2;
    }

    #[inline]
    pub(super) fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

#[derive(Clone, Copy)]
pub enum BandShape {
    Lpf,
    Hpf,
}

/// RBJ cookbook coefficients.
pub fn biquad_for(shape: BandShape, freq_hz: f32, q: f32, sample_rate: u32) -> Biquad {
    let fs = sample_rate as f32;
    let w0 = 2.0 * std::f32::consts::PI * (freq_hz.max(1.0) / fs);
    let (sinw, cosw) = (w0.sin(), w0.cos());
    let q = q.max(0.05);
    let alpha = sinw / (2.0 * q);

    let (b0, b1, b2, a0, a1, a2) = match shape {
        BandShape::Lpf => (
            (1.0 - cosw) * 0.5,
            1.0 - cosw,
            (1.0 - cosw) * 0.5,
            1.0 + alpha,
            -2.0 * cosw,
            1.0 - alpha,
        ),
        BandShape::Hpf => (
            (1.0 + cosw) * 0.5,
            -(1.0 + cosw),
            (1.0 + cosw) * 0.5,
            1.0 + alpha,
            -2.0 * cosw,
            1.0 - alpha,
        ),
    };
    let inv = 1.0 / a0;
    Biquad {
        b0: b0 * inv,
        b1: b1 * inv,
        b2: b2 * inv,
        a1: a1 * inv,
        a2: a2 * inv,
        z1: 0.0,
        z2: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    // Steady-state amplitude of a sine at `freq` after the filter settles.
    fn gain_at(mut f: Biquad, freq: f32) -> f32 {
        let mut peak = 0.0f32;
        for n in 0..SR as usize {
            let x = (std::f32::consts::TAU * freq * n as f32 / SR as f32).sin();
            let y = f.process(x);
            if n > SR as usize / 2 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn lowpass_passes_lows_and_cuts_highs() {
        let f = biquad_for(BandShape::Lpf, 1_000.0, 0.707, SR);
        assert!((gain_at(f, 100.0) - 1.0).abs() < 0.02);
        assert!(gain_at(f, 10_000.0) < 0.02);
    }

    #[test]
    fn highpass_passes_highs_and_cuts_lows() {
        let f = biquad_for(BandShape::Hpf, 1_000.0, 0.707, SR);
        assert!((gain_at(f, 10_000.0) - 1.0).abs() < 0.02);
        assert!(gain_at(f, 50.0) < 0.01);
    }

    #[test]
    fn butterworth_q_is_minus_3db_at_cutoff() {
        let f = biquad_for(BandShape::Lpf, 1_000.0, std::f32::consts::FRAC_1_SQRT_2, SR);
        assert!((gain_at(f, 1_000.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    }

    #[test]
    fn degenerate_params_stay_finite() {
        let mut f = biquad_for(BandShape::Lpf, 0.0, 0.0, SR);
        for _ in 0..1_000 {
            assert!(f.process(1.0).is_finite());
        }
    }

    #[test]
    fn retune_keeps_state() {
        let mut f = biquad_for(BandShape::Lpf, 1_000.0, 0.707, SR);
        for _ in 0..100 {
            f.process(1.0);
        }
        let mut retuned = f;
        retuned.retune(biquad_for(BandShape::Lpf, 1_000.0, 0.707, SR));
        assert_eq!(
            retuned.process(1.0),
            f.process(1.0),
            "same coefficients, same output"
        );
    }
}
