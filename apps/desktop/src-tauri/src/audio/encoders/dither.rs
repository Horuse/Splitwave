pub(super) struct Xorshift {
    s: u64,
}

impl Xorshift {
    pub(super) fn seed(s: u64) -> Self {
        Self { s }
    }

    #[inline]
    fn next_u32(&mut self) -> u32 {
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.s = x;
        x as u32
    }

    /// Triangular PDF noise in (-1, 1) — sum of two uniforms.
    #[inline]
    pub(super) fn tpdf(&mut self) -> f32 {
        let a = (self.next_u32() as f32) / (u32::MAX as f32) - 0.5;
        let b = (self.next_u32() as f32) / (u32::MAX as f32) - 0.5;
        a + b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tpdf_stays_within_one_lsb_and_centres_on_zero() {
        let mut rng = Xorshift::seed(0x9e3779b97f4a7c15);
        let n = 200_000;
        let mut sum = 0.0f64;
        for _ in 0..n {
            let v = rng.tpdf();
            assert!(v > -1.0 && v < 1.0, "{v}");
            sum += v as f64;
        }
        assert!((sum / n as f64).abs() < 0.01, "mean {}", sum / n as f64);
    }

    #[test]
    fn same_seed_same_sequence() {
        let (mut a, mut b) = (Xorshift::seed(42), Xorshift::seed(42));
        for _ in 0..100 {
            assert_eq!(a.tpdf(), b.tpdf());
        }
    }
}
