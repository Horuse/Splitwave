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
    /// Identity pass-through filter (H(z) = 1).
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Copy another biquad's coefficients while keeping this one's state — lets a
    /// filter be retuned live without a discontinuity.
    #[inline]
    pub fn retune(&mut self, c: Biquad) {
        self.b0 = c.b0;
        self.b1 = c.b1;
        self.b2 = c.b2;
        self.a1 = c.a1;
        self.a2 = c.a2;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
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

/// RBJ cookbook peaking EQ filter.
/// At 0 dB gain, acts as an exact identity pass-through.
pub fn biquad_peaking(freq_hz: f32, q: f32, gain_db: f32, sample_rate: u32) -> Biquad {
    if gain_db.abs() < 1e-4 {
        return Biquad::identity();
    }
    let fs = sample_rate as f32;
    let w0 = 2.0 * std::f32::consts::PI * (freq_hz.clamp(10.0, fs * 0.49) / fs);
    let (sinw, cosw) = (w0.sin(), w0.cos());
    let a = 10.0_f32.powf(gain_db / 40.0);
    let q = q.max(0.05);
    let alpha = sinw / (2.0 * q);

    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cosw;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cosw;
    let a2 = 1.0 - alpha / a;

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
