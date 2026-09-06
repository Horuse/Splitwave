//! Wire format for direct-IP audio. Each UDP datagram is a 4-byte header
//! followed by one payload: `[format][channel][seq_be_hi][seq_be_lo]`.
//!
//! `seq` is a per-channel packet counter for loss/reorder detection. Audio is
//! always carried at 48 kHz stereo regardless of `format`, so the receiver is
//! format-agnostic beyond decoding the payload.

pub const HEADER_LEN: usize = 4;
/// Keep datagrams under a typical MTU so PCM isn't IP-fragmented.
pub const MAX_PAYLOAD: usize = 1200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    PcmF32,
    PcmI16,
    Opus,
}

impl Format {
    pub fn to_byte(self) -> u8 {
        match self {
            Format::PcmF32 => 0,
            Format::PcmI16 => 1,
            Format::Opus => 2,
        }
    }

    pub fn from_byte(b: u8) -> Option<Format> {
        match b {
            0 => Some(Format::PcmF32),
            1 => Some(Format::PcmI16),
            2 => Some(Format::Opus),
            _ => None,
        }
    }
}

pub const HEADER_LEN_BASE: usize = 8;
pub const HEADER_LEN_EXT: usize = 12;

/// Bit 7: set if packet includes the 8-byte base extended header (`sample_rate: u32`).
pub const FLAG_EXTENDED: u8 = 0x80;
/// Bit 6: set if packet includes the 4-byte codec metadata extension (`codec_param`).
pub const FLAG_CODEC_META: u8 = 0x40;

pub struct Parsed<'a> {
    pub format: Format,
    pub channel: u8,
    pub seq: u16,
    pub sample_rate: u32,
    pub opus_bitrate_kbps: Option<u16>,
    pub opus_app: Option<u8>,
    pub payload: &'a [u8],
}

/// Writes the self-describing header into `buf` (cleared first); the caller appends the payload.
/// If `codec_param` is provided (e.g. for Opus: bitrate + application mode), writes a 12-byte header with FLAG_CODEC_META.
/// Otherwise (e.g. for PCM), writes a compact 8-byte header with only FLAG_EXTENDED.
pub fn write_header(
    buf: &mut Vec<u8>,
    format: Format,
    channel: u8,
    seq: u16,
    sample_rate: u32,
    codec_param: Option<(u16, u8)>,
) {
    buf.clear();
    let has_codec_meta = codec_param.is_some();
    let mut b0 = FLAG_EXTENDED | format.to_byte();
    if has_codec_meta {
        b0 |= FLAG_CODEC_META;
    }
    buf.push(b0);
    buf.push(channel);
    buf.extend_from_slice(&seq.to_be_bytes());
    buf.extend_from_slice(&sample_rate.to_be_bytes());
    if let Some((p16, p8)) = codec_param {
        buf.extend_from_slice(&p16.to_be_bytes());
        buf.push(p8);
        buf.push(0); // reserved
    }
}

pub fn parse(data: &[u8]) -> Option<Parsed<'_>> {
    if data.len() < HEADER_LEN {
        return None;
    }
    let b0 = data[0];
    if b0 & FLAG_EXTENDED != 0 {
        if data.len() < HEADER_LEN_BASE {
            return None;
        }
        let format = Format::from_byte(b0 & 0x3F)?;
        let channel = data[1];
        let seq = u16::from_be_bytes([data[2], data[3]]);
        let sample_rate = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

        let (opus_bitrate_kbps, opus_app, header_len) = if b0 & FLAG_CODEC_META != 0 {
            if data.len() < HEADER_LEN_EXT {
                return None;
            }
            let kbps = u16::from_be_bytes([data[8], data[9]]);
            let app = data[10];
            let kbps_opt = if kbps > 0 { Some(kbps) } else { None };
            let app_opt = if app > 0 { Some(app) } else { None };
            (kbps_opt, app_opt, HEADER_LEN_EXT)
        } else {
            (None, None, HEADER_LEN_BASE)
        };

        Some(Parsed {
            format,
            channel,
            seq,
            sample_rate,
            opus_bitrate_kbps,
            opus_app,
            payload: &data[header_len..],
        })
    } else {
        let format = Format::from_byte(b0)?;
        let channel = data[1];
        let seq = u16::from_be_bytes([data[2], data[3]]);
        Some(Parsed {
            format,
            channel,
            seq,
            sample_rate: 48_000,
            opus_bitrate_kbps: None,
            opus_app: None,
            payload: &data[HEADER_LEN..],
        })
    }
}

/// Interleaved f32 samples -> little-endian bytes.
pub fn pcm_f32_encode(samples: &[f32], out: &mut Vec<u8>) {
    out.clear();
    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
}

/// Interleaved f32 samples -> little-endian i16 bytes (clamped).
pub fn pcm_i16_encode(samples: &[f32], out: &mut Vec<u8>) {
    out.clear();
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// LE f32 bytes -> interleaved f32 samples (appended to `out`).
pub fn pcm_f32_decode(payload: &[u8], out: &mut Vec<f32>) {
    for chunk in payload.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
}

/// LE i16 bytes -> interleaved f32 samples (appended to `out`).
pub fn pcm_i16_decode(payload: &[u8], out: &mut Vec<f32>) {
    for chunk in payload.chunks_exact(2) {
        let v = i16::from_le_bytes([chunk[0], chunk[1]]);
        out.push(v as f32 / i16::MAX as f32);
    }
}
