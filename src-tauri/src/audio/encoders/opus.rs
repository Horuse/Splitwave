//! Opus encoder muxed into an OGG container (RFC 7845). Page-level CRC means
//! a truncated tail page is dropped by readers — periodic `flush` pushes
//! complete pages to disk for crash-safety at flush granularity.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opus::{Application as OpusApp, Bitrate, Channels, Encoder};

use super::AudioEncoder;
use crate::audio::graph::OpusApplication;
use crate::error::{AppError, AppResult};

/// 20 ms @ 48 kHz — WebRTC default; best quality/latency trade-off.
const FRAME_SAMPLES: usize = 960;
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS_U8: u8 = 2;
/// libopus recommends ≤4000 bytes per non-multistream packet.
const MAX_PACKET_BYTES: usize = 4000;

pub struct OpusRecorder {
	writer: PacketWriter<'static, BufWriter<File>>,
	encoder: Encoder,
	serial: u32,
	granule: u64,
	pending: Vec<f32>,
	encode_buf: Vec<u8>,
}

impl OpusRecorder {
	pub fn create(
		path: &Path,
		application: OpusApplication,
		bitrate_bps: u32,
	) -> AppResult<Self> {
		let opus_app = match application {
			OpusApplication::Audio => OpusApp::Audio,
			OpusApplication::Voip => OpusApp::Voip,
			OpusApplication::LowDelay => OpusApp::LowDelay,
		};
		let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Stereo, opus_app)
			.map_err(|e| AppError::Stream(format!("opus init: {e}")))?;
		encoder
			.set_bitrate(Bitrate::Bits(bitrate_bps.clamp(6_000, 510_000) as i32))
			.map_err(|e| AppError::Stream(format!("opus bitrate: {e}")))?;

		let lookahead = encoder
			.get_lookahead()
			.ok()
			.and_then(|n| u16::try_from(n).ok())
			.unwrap_or(312);

		let file = File::create(path)
			.map_err(|e| AppError::Stream(format!("create {}: {e}", path.display())))?;
		let mut writer = PacketWriter::new(BufWriter::new(file));
		let serial = generate_serial();

		write_opus_head(&mut writer, serial, lookahead)?;
		write_opus_tags(&mut writer, serial)?;

		Ok(Self {
			writer,
			encoder,
			serial,
			granule: 0,
			pending: Vec::with_capacity(FRAME_SAMPLES * 2 * 2),
			encode_buf: vec![0u8; MAX_PACKET_BYTES],
		})
	}

	fn encode_one_frame(&mut self, frame: &[f32], end: PacketWriteEndInfo) -> AppResult<()> {
		let n = self
			.encoder
			.encode_float(frame, &mut self.encode_buf)
			.map_err(|e| AppError::Stream(format!("opus encode: {e}")))?;
		self.granule += FRAME_SAMPLES as u64;
		let packet: Vec<u8> = self.encode_buf[..n].to_vec();
		self.writer
			.write_packet(packet, self.serial, end, self.granule)
			.map_err(|e| AppError::Stream(format!("ogg write: {e}")))
	}
}

impl AudioEncoder for OpusRecorder {
	fn write_stereo(&mut self, samples: &[f32]) -> AppResult<()> {
		debug_assert!(samples.len() % 2 == 0, "stereo buffer must be even length");
		self.pending.extend_from_slice(samples);

		let frame_interleaved = FRAME_SAMPLES * 2;
		while self.pending.len() >= frame_interleaved {
			// Borrow checker: take frame as owned slice copy to release pending borrow.
			let frame: Vec<f32> = self.pending[..frame_interleaved].to_vec();
			self.encode_one_frame(&frame, PacketWriteEndInfo::EndPage)?;
			self.pending.drain(..frame_interleaved);
		}
		Ok(())
	}

	fn flush(&mut self) -> AppResult<()> {
		self.writer
			.inner_mut()
			.flush()
			.map_err(|e| AppError::Stream(format!("flush opus: {e}")))
	}

	fn finalize(self: Box<Self>) -> AppResult<()> {
		let Self {
			mut writer,
			mut encoder,
			serial,
			mut granule,
			pending,
			mut encode_buf,
		} = *self;

		let frame_interleaved = FRAME_SAMPLES * 2;
		if !pending.is_empty() {
			let mut padded = pending;
			padded.resize(frame_interleaved, 0.0);
			let n = encoder
				.encode_float(&padded, &mut encode_buf)
				.map_err(|e| AppError::Stream(format!("opus final encode: {e}")))?;
			granule += FRAME_SAMPLES as u64;
			let packet: Vec<u8> = encode_buf[..n].to_vec();
			writer
				.write_packet(packet, serial, PacketWriteEndInfo::EndStream, granule)
				.map_err(|e| AppError::Stream(format!("ogg final write: {e}")))?;
		} else {
			// Emit a zero-length terminator packet to mark EOS on the stream.
			writer
				.write_packet(Vec::<u8>::new(), serial, PacketWriteEndInfo::EndStream, granule)
				.map_err(|e| AppError::Stream(format!("ogg eos: {e}")))?;
		}

		writer
			.inner_mut()
			.flush()
			.map_err(|e| AppError::Stream(format!("opus finalize flush: {e}")))
	}
}

fn write_opus_head<W: Write>(
	writer: &mut PacketWriter<'_, W>,
	serial: u32,
	pre_skip: u16,
) -> AppResult<()> {
	let mut buf = Vec::with_capacity(19);
	buf.extend_from_slice(b"OpusHead");
	buf.push(1); // version
	buf.push(CHANNELS_U8);
	buf.extend_from_slice(&pre_skip.to_le_bytes());
	buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
	buf.extend_from_slice(&0i16.to_le_bytes()); // output gain Q7.8 = 0 dB
	buf.push(0); // channel mapping family 0 (mono/stereo)

	writer
		.write_packet(buf, serial, PacketWriteEndInfo::EndPage, 0)
		.map_err(|e| AppError::Stream(format!("ogg head: {e}")))
}

fn write_opus_tags<W: Write>(
	writer: &mut PacketWriter<'_, W>,
	serial: u32,
) -> AppResult<()> {
	const VENDOR: &[u8] = b"BetterAudio";
	let mut buf = Vec::with_capacity(8 + 4 + VENDOR.len() + 4);
	buf.extend_from_slice(b"OpusTags");
	buf.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
	buf.extend_from_slice(VENDOR);
	buf.extend_from_slice(&0u32.to_le_bytes()); // 0 user comments

	writer
		.write_packet(buf, serial, PacketWriteEndInfo::EndPage, 0)
		.map_err(|e| AppError::Stream(format!("ogg tags: {e}")))
}

fn generate_serial() -> u32 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_nanos() as u32)
		.unwrap_or(0xDEAD_BEEF)
}
