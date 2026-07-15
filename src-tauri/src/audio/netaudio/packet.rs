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

pub struct Parsed<'a> {
    pub format: Format,
    pub channel: u8,
    pub seq: u16,
    pub payload: &'a [u8],
}

/// Writes the header into `buf` (cleared first); the caller appends the payload.
pub fn write_header(buf: &mut Vec<u8>, format: Format, channel: u8, seq: u16) {
    buf.clear();
    buf.push(format.to_byte());
    buf.push(channel);
    buf.extend_from_slice(&seq.to_be_bytes());
}

pub fn parse(data: &[u8]) -> Option<Parsed<'_>> {
    if data.len() < HEADER_LEN {
        return None;
    }
    Some(Parsed {
        format: Format::from_byte(data[0])?,
        channel: data[1],
        seq: u16::from_be_bytes([data[2], data[3]]),
        payload: &data[HEADER_LEN..],
    })
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
