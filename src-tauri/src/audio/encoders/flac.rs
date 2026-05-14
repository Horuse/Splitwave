//! FLAC encoder via `flac-codec`. Lossless compressed; STREAMINFO is patched
//! at finalize, so a mid-recording crash leaves placeholder header values —
//! most players cope, but use WAV if strict validators matter.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use flac_codec::encode::{FlacSampleWriter, Options as FlacOptions};

use super::dither::Xorshift;
use super::AudioEncoder;
use crate::audio::graph::{FlacBitDepth, FlacCompression};
use crate::error::{AppError, AppResult};

pub struct FlacRecorder {
	writer: FlacSampleWriter<BufWriter<File>>,
	max_sample: f32,
	min_sample: f32,
	dither: Xorshift,
	scratch: Vec<i32>,
}

impl FlacRecorder {
	pub fn create(
		path: &Path,
		sample_rate: u32,
		bit_depth: FlacBitDepth,
		compression: FlacCompression,
	) -> AppResult<Self> {
		let bps: u32 = match bit_depth {
			FlacBitDepth::I16 => 16,
			FlacBitDepth::I24 => 24,
		};
		let options = match compression {
			FlacCompression::Fast => FlacOptions::fast(),
			FlacCompression::Default => FlacOptions::default(),
			FlacCompression::Best => FlacOptions::best(),
		};
		let writer = FlacSampleWriter::create(path, options, sample_rate, bps, 2, None)
			.map_err(|e| AppError::Stream(format!("flac create {}: {e}", path.display())))?;
		let max_sample = ((1u32 << (bps - 1)) - 1) as f32;
		let min_sample = -((1u32 << (bps - 1)) as f32);
		Ok(Self {
			writer,
			max_sample,
			min_sample,
			dither: Xorshift::seed(0x9e3779b97f4a7c15),
			scratch: Vec::with_capacity(2048),
		})
	}
}

impl AudioEncoder for FlacRecorder {
	fn write_stereo(&mut self, samples: &[f32]) -> AppResult<()> {
		debug_assert!(samples.len() % 2 == 0, "stereo buffer must be even length");
		self.scratch.clear();
		if self.scratch.capacity() < samples.len() {
			self.scratch.reserve(samples.len() - self.scratch.capacity());
		}
		for &s in samples {
			let dithered = s * self.max_sample + self.dither.tpdf();
			let q = dithered.clamp(self.min_sample, self.max_sample).round() as i32;
			self.scratch.push(q);
		}
		self.writer
			.write(&self.scratch)
			.map_err(|e| AppError::Stream(format!("flac write: {e}")))
	}

	fn flush(&mut self) -> AppResult<()> {
		Ok(())
	}

	fn finalize(self: Box<Self>) -> AppResult<()> {
		let Self { writer, .. } = *self;
		writer
			.finalize()
			.map_err(|e| AppError::Stream(format!("flac finalize: {e}")))
	}
}
