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
