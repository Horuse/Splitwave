//! AIFF (Apple's RIFF cousin): big-endian PCM int (16/24-bit). Each `flush`
//! patches FORM/COMM/SSND size + frame count → file on disk is a valid AIFF
//! at the last flush boundary.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use super::dither::Xorshift;
use super::AudioEncoder;
use crate::audio::graph::AiffBitDepth;
use crate::error::{AppError, AppResult};

const CHANNELS: u16 = 2;
const HEADER_SIZE: u64 = 54;
const OFFSET_FORM_SIZE: u64 = 4;
const OFFSET_NUM_FRAMES: u64 = 22;
const OFFSET_SSND_SIZE: u64 = 42;

pub struct AiffRecorder {
	inner: BufWriter<File>,
	samples_per_channel: u64,
	bit_depth: AiffBitDepth,
	dither: Xorshift,
}

impl AiffRecorder {
	pub fn create(path: &Path, sample_rate: u32, bit_depth: AiffBitDepth) -> AppResult<Self> {
		let file = File::create(path)
			.map_err(|e| AppError::Stream(format!("create {}: {e}", path.display())))?;
		let mut inner = BufWriter::new(file);
		write_header(&mut inner, sample_rate, bit_depth, 0)
			.map_err(|e| AppError::Stream(format!("write aiff header: {e}")))?;
		Ok(Self {
			inner,
			samples_per_channel: 0,
			bit_depth,
			dither: Xorshift::seed(0x9e3779b97f4a7c15),
		})
	}

	fn bytes_per_sample(&self) -> usize {
		match self.bit_depth {
			AiffBitDepth::I16 => 2,
			AiffBitDepth::I24 => 3,
		}
	}

	fn write_pcm(&mut self, samples: &[f32]) -> AppResult<()> {
		let (max, min) = match self.bit_depth {
			AiffBitDepth::I16 => (32_767.0_f32, -32_768.0_f32),
			AiffBitDepth::I24 => (8_388_607.0_f32, -8_388_608.0_f32),
		};
		let bps = self.bytes_per_sample();
		let mut buf = [0u8; 8];
		for pair in samples.chunks_exact(2) {
			for (i, &s) in pair.iter().enumerate() {
				let dithered = s * max + self.dither.tpdf();
				let q = dithered.clamp(min, max).round() as i32;
				let be = q.to_be_bytes();
				// `i32::to_be_bytes` = [MSB, ..., LSB]; for i16 we need the last
				// 2 bytes, for i24 the last 3.
				buf[i * bps..(i + 1) * bps].copy_from_slice(&be[4 - bps..4]);
			}
			self.inner
				.write_all(&buf[..bps * 2])
				.map_err(|e| AppError::Stream(format!("write aiff: {e}")))?;
		}
		Ok(())
	}
}

impl AudioEncoder for AiffRecorder {
	fn write_stereo(&mut self, samples: &[f32]) -> AppResult<()> {
		debug_assert!(samples.len() % 2 == 0, "stereo buffer must be even length");
		self.write_pcm(samples)?;
		self.samples_per_channel += (samples.len() / 2) as u64;
		Ok(())
	}

	fn flush(&mut self) -> AppResult<()> {
		self.inner
			.flush()
			.map_err(|e| AppError::Stream(format!("flush aiff: {e}")))?;

		let bps = self.bytes_per_sample() as u64;
		let data_size = self.samples_per_channel * (CHANNELS as u64) * bps;
		let ssnd_body = 8u64.saturating_add(data_size);
		let form_size = (HEADER_SIZE - 8).saturating_add(data_size);
		// AIFF chunk sizes are u32 — saturate (≈6 h of 24-bit stereo at 48 k).
		let form_size_u32 = u32::try_from(form_size).unwrap_or(u32::MAX);
		let num_frames_u32 = u32::try_from(self.samples_per_channel).unwrap_or(u32::MAX);
		let ssnd_size_u32 = u32::try_from(ssnd_body).unwrap_or(u32::MAX);

		let file = self.inner.get_mut();
		file.seek(SeekFrom::Start(OFFSET_FORM_SIZE))
			.map_err(|e| AppError::Stream(format!("seek aiff: {e}")))?;
		file.write_all(&form_size_u32.to_be_bytes())
			.map_err(|e| AppError::Stream(format!("patch form size: {e}")))?;
		file.seek(SeekFrom::Start(OFFSET_NUM_FRAMES))
			.map_err(|e| AppError::Stream(format!("seek aiff: {e}")))?;
		file.write_all(&num_frames_u32.to_be_bytes())
			.map_err(|e| AppError::Stream(format!("patch num frames: {e}")))?;
		file.seek(SeekFrom::Start(OFFSET_SSND_SIZE))
			.map_err(|e| AppError::Stream(format!("seek aiff: {e}")))?;
		file.write_all(&ssnd_size_u32.to_be_bytes())
			.map_err(|e| AppError::Stream(format!("patch ssnd size: {e}")))?;
		file.seek(SeekFrom::End(0))
			.map_err(|e| AppError::Stream(format!("seek aiff end: {e}")))?;
		Ok(())
	}

	fn finalize(mut self: Box<Self>) -> AppResult<()> {
		self.flush()
	}
}

fn write_header(
	w: &mut impl Write,
	sample_rate: u32,
	bit_depth: AiffBitDepth,
	num_frames: u32,
) -> std::io::Result<()> {
	let bits: u16 = match bit_depth {
		AiffBitDepth::I16 => 16,
		AiffBitDepth::I24 => 24,
	};
	let bps = (bits / 8) as u32;
	let data_size = num_frames.saturating_mul((CHANNELS as u32) * bps);
	let form_size = (HEADER_SIZE as u32 - 8).saturating_add(data_size);
	let ssnd_size = 8u32.saturating_add(data_size);

	w.write_all(b"FORM")?;
	w.write_all(&form_size.to_be_bytes())?;
	w.write_all(b"AIFF")?;

	w.write_all(b"COMM")?;
	w.write_all(&18u32.to_be_bytes())?;
	w.write_all(&(CHANNELS as i16).to_be_bytes())?;
	w.write_all(&num_frames.to_be_bytes())?;
	w.write_all(&(bits as i16).to_be_bytes())?;
	w.write_all(&sample_rate_to_extended_80(sample_rate))?;

	w.write_all(b"SSND")?;
	w.write_all(&ssnd_size.to_be_bytes())?;
	w.write_all(&0u32.to_be_bytes())?; // offset
	w.write_all(&0u32.to_be_bytes())?; // block size
	Ok(())
}

/// AIFF stores sample rate as IEEE 754 80-bit extended (1 sign + 15 exponent
/// + 64 fraction with explicit MSB). Exponent bias 16383.
fn sample_rate_to_extended_80(rate: u32) -> [u8; 10] {
	let mut out = [0u8; 10];
	if rate == 0 {
		return out;
	}
	let mut mantissa = rate as u64;
	let mut shift = 0u32;
	while (mantissa & (1u64 << 63)) == 0 {
		mantissa <<= 1;
		shift += 1;
	}
	let exponent: u16 = (16383 + 63 - shift) as u16;
	out[0..2].copy_from_slice(&exponent.to_be_bytes());
	out[2..10].copy_from_slice(&mantissa.to_be_bytes());
	out
}
