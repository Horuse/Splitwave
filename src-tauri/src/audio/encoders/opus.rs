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
/// libopus recommends ≤4000 bytes per non-multistream packet.
const MAX_PACKET_BYTES: usize = 4000;

pub struct OpusRecorder {
    writer: PacketWriter<'static, BufWriter<File>>,
    encoder: Encoder,
    serial: u32,
    granule: u64,
    input_frames: u64,
    pre_skip: u16,
    pending: Vec<f32>,
    encode_buf: Vec<u8>,
    channels: u16,
}

impl OpusRecorder {
    pub fn create(
        path: &Path,
        channels: u16,
        application: OpusApplication,
        bitrate_bps: u32,
    ) -> AppResult<Self> {
        let opus_app = match application {
            OpusApplication::Audio => OpusApp::Audio,
            OpusApplication::Voip => OpusApp::Voip,
            OpusApplication::LowDelay => OpusApp::LowDelay,
        };
        let layout = if channels == 1 {
            Channels::Mono
        } else {
            Channels::Stereo
        };
        let mut encoder = Encoder::new(SAMPLE_RATE, layout, opus_app)
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

        write_opus_head(&mut writer, serial, channels as u8, lookahead)?;
        write_opus_tags(&mut writer, serial)?;

        Ok(Self {
            writer,
            encoder,
            serial,
            granule: 0,
            input_frames: 0,
            pre_skip: lookahead,
            pending: Vec::with_capacity(FRAME_SAMPLES * 2 * channels as usize),
            encode_buf: vec![0u8; MAX_PACKET_BYTES],
            channels,
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
    fn write_interleaved(&mut self, samples: &[f32]) -> AppResult<()> {
        super::validate_interleaved(samples, self.channels)?;
        self.input_frames += (samples.len() / self.channels as usize) as u64;
        self.pending.extend_from_slice(samples);

        let frame_interleaved = FRAME_SAMPLES * self.channels as usize;
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
            channels,
            serial,
            mut granule,
            input_frames,
            pre_skip,
            pending,
            mut encode_buf,
        } = *self;

        let frame_interleaved = FRAME_SAMPLES * channels as usize;
        let target_granule = input_frames + pre_skip as u64;
        let mut padded = pending;
        while granule < target_granule {
            padded.resize(frame_interleaved, 0.0);
            let n = encoder
                .encode_float(&padded, &mut encode_buf)
                .map_err(|e| AppError::Stream(format!("opus final encode: {e}")))?;
            let packet_end = granule + FRAME_SAMPLES as u64;
            let last = packet_end >= target_granule;
            granule = if last { target_granule } else { packet_end };
            let packet: Vec<u8> = encode_buf[..n].to_vec();
            writer
                .write_packet(
                    packet,
                    serial,
                    if last {
                        PacketWriteEndInfo::EndStream
                    } else {
                        PacketWriteEndInfo::EndPage
                    },
                    granule,
                )
                .map_err(|e| AppError::Stream(format!("ogg final write: {e}")))?;
            padded.fill(0.0);
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
    channels: u8,
    pre_skip: u16,
) -> AppResult<()> {
    let mut buf = Vec::with_capacity(19);
    buf.extend_from_slice(b"OpusHead");
    buf.push(1); // version
    buf.push(channels);
    buf.extend_from_slice(&pre_skip.to_le_bytes());
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    buf.extend_from_slice(&0i16.to_le_bytes()); // output gain Q7.8 = 0 dB
    buf.push(0); // channel mapping family 0 (mono/stereo)

    writer
        .write_packet(buf, serial, PacketWriteEndInfo::EndPage, 0)
        .map_err(|e| AppError::Stream(format!("ogg head: {e}")))
}

fn write_opus_tags<W: Write>(writer: &mut PacketWriter<'_, W>, serial: u32) -> AppResult<()> {
    const VENDOR: &[u8] = b"Splitwave";
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::encoders::AudioEncoder;
    use crate::audio::graph::OpusApplication;
    use ogg::reading::PacketReader;

    fn temp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("opus_enc_test_{}_{}", std::process::id(), name));
        p
    }

    fn packets(path: &Path) -> Vec<ogg::Packet> {
        let file = File::open(path).expect("open ogg");
        let mut reader = PacketReader::new(file);
        let mut packets = Vec::new();
        while let Some(packet) = reader.read_packet().expect("valid ogg") {
            packets.push(packet);
        }
        packets
    }

    fn pre_skip(head: &ogg::Packet) -> u64 {
        assert_eq!(&head.data[..8], b"OpusHead");
        u16::from_le_bytes([head.data[10], head.data[11]]) as u64
    }

    #[test]
    fn empty_stream_has_valid_trimmed_eos() {
        let path = temp("empty.opus");
        let enc = OpusRecorder::create(&path, 2, OpusApplication::Audio, 96_000).expect("create");
        Box::new(enc).finalize().expect("finalize");
        let packets = packets(&path);
        assert_eq!(&packets[1].data[..8], b"OpusTags");
        let last = packets.last().expect("audio eos packet");
        assert!(last.last_in_stream());
        assert_eq!(last.absgp_page(), pre_skip(&packets[0]));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mono_stream_records_exact_unaligned_duration() {
        let path = temp("mono.opus");
        let mut enc =
            OpusRecorder::create(&path, 1, OpusApplication::Audio, 96_000).expect("create mono");
        let frames = 1_337usize;
        let samples: Vec<f32> = (0..frames)
            .map(|i| 0.5 * (i as f32 * 440.0 * 6.28 / 48_000.0).sin())
            .collect();
        enc.write_interleaved(&samples[..517]).expect("first write");
        enc.write_interleaved(&samples[517..])
            .expect("second write");
        Box::new(enc).finalize().expect("finalize");
        let packets = packets(&path);
        assert_eq!(packets[0].data[9], 1, "mono OpusHead");
        let last = packets.last().expect("eos");
        assert!(last.last_in_stream());
        assert_eq!(last.absgp_page() - pre_skip(&packets[0]), frames as u64);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stereo_writes_must_end_on_complete_frames() {
        let path = temp("odd.opus");
        let mut enc =
            OpusRecorder::create(&path, 2, OpusApplication::Audio, 96_000).expect("create");
        let err = enc
            .write_interleaved(&[0.0, 0.0, 1.0])
            .expect_err("partial stereo frame must be rejected");
        assert!(format!("{err}").contains("3 samples for 2 channels"));
        let _ = std::fs::remove_file(&path);
    }
}
