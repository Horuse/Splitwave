//! Wire format for direct-IP audio.
//!
//! ### Protocol v2:
//! - Base header (9 bytes, for PCM):
//!   `[version: 0x82][format: 1B][channel: 1B][seq: 2B BE][sample_rate: 4B BE]`
//! - Extended header (12 bytes, for Opus only):
//!   `[version: 0x82][format: 1B][channel: 1B][seq: 2B BE][sample_rate: 4B BE][bitrate_kbps: 2B BE][opus_app: 1B]`
//!
//! ### Protocol v1 (legacy fallback):
//! - Fixed 4 bytes: `[format: 1B][channel: 1B][seq: 2B BE]` (assumes 48 kHz stereo).

pub const HEADER_LEN_V1: usize = 4;
pub const HEADER_LEN_V2_BASE: usize = 9;
pub const HEADER_LEN_V2_OPUS: usize = 12;

/// Protocol version 2 marker byte (`0x82`). Distinct from legacy v1 format bytes (0, 1, 2).
pub const PROTOCOL_V2: u8 = 0x82;

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
    pub sample_rate: u32,
    pub opus_bitrate_kbps: Option<u16>,
    pub opus_app: Option<u8>,
    pub payload: &'a [u8],
}

/// Writes the self-describing header into `buf` (cleared first); the caller appends the payload.
/// - For PCM: writes 9 bytes `[PROTOCOL_V2, format, channel, seq_be, sample_rate_be]`.
/// - For Opus: appends 3 bytes `[bitrate_kbps_be, opus_app]` (12 bytes total).
pub fn write_header(
    buf: &mut Vec<u8>,
    format: Format,
    channel: u8,
    seq: u16,
    sample_rate: u32,
    opus_bitrate_kbps: u16,
    opus_app: u8,
) {
    buf.clear();
    buf.push(PROTOCOL_V2);
    buf.push(format.to_byte());
    buf.push(channel);
    buf.extend_from_slice(&seq.to_be_bytes());
    buf.extend_from_slice(&sample_rate.to_be_bytes());
    if format == Format::Opus {
        buf.extend_from_slice(&opus_bitrate_kbps.to_be_bytes());
        buf.push(opus_app);
    }
}

pub fn parse(data: &[u8]) -> Option<Parsed<'_>> {
    if data.len() < HEADER_LEN_V1 {
        return None;
    }
    if data[0] == PROTOCOL_V2 {
        if data.len() < HEADER_LEN_V2_BASE {
            return None;
        }
        let format = Format::from_byte(data[1])?;
        let channel = data[2];
        let seq = u16::from_be_bytes([data[3], data[4]]);
        let sample_rate = u32::from_be_bytes([data[5], data[6], data[7], data[8]]);

        let (opus_bitrate_kbps, opus_app, header_len) = if format == Format::Opus {
            if data.len() < HEADER_LEN_V2_OPUS {
                return None;
            }
            let kbps = u16::from_be_bytes([data[9], data[10]]);
            let app = data[11];
            let kbps_opt = if kbps > 0 { Some(kbps) } else { None };
            let app_opt = if app > 0 { Some(app) } else { None };
            (kbps_opt, app_opt, HEADER_LEN_V2_OPUS)
        } else {
            (None, None, HEADER_LEN_V2_BASE)
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
        // Legacy v1 header: [format, channel, seq_be]
        let format = Format::from_byte(data[0])?;
        let channel = data[1];
        let seq = u16::from_be_bytes([data[2], data[3]]);
        Some(Parsed {
            format,
            channel,
            seq,
            sample_rate: 48_000,
            opus_bitrate_kbps: None,
            opus_app: None,
            payload: &data[HEADER_LEN_V1..],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v2_pcm_roundtrip() {
        let mut buf = Vec::new();
        write_header(&mut buf, Format::PcmF32, 1, 42, 96_000, 0, 0);
        assert_eq!(buf.len(), HEADER_LEN_V2_BASE);
        buf.extend_from_slice(&[1, 2, 3, 4]);

        let parsed = parse(&buf).expect("should parse v2 pcm");
        assert_eq!(parsed.format, Format::PcmF32);
        assert_eq!(parsed.channel, 1);
        assert_eq!(parsed.seq, 42);
        assert_eq!(parsed.sample_rate, 96_000);
        assert_eq!(parsed.opus_bitrate_kbps, None);
        assert_eq!(parsed.opus_app, None);
        assert_eq!(parsed.payload, &[1, 2, 3, 4]);
    }

    #[test]
    fn test_v2_opus_roundtrip() {
        let mut buf = Vec::new();
        write_header(&mut buf, Format::Opus, 0, 100, 48_000, 128, 3);
        assert_eq!(buf.len(), HEADER_LEN_V2_OPUS);
        buf.extend_from_slice(&[0xFA, 0xFB]);

        let parsed = parse(&buf).expect("should parse v2 opus");
        assert_eq!(parsed.format, Format::Opus);
        assert_eq!(parsed.channel, 0);
        assert_eq!(parsed.seq, 100);
        assert_eq!(parsed.sample_rate, 48_000);
        assert_eq!(parsed.opus_bitrate_kbps, Some(128));
        assert_eq!(parsed.opus_app, Some(3));
        assert_eq!(parsed.payload, &[0xFA, 0xFB]);
    }

    #[test]
    fn test_v1_legacy_fallback() {
        let buf = vec![Format::PcmF32.to_byte(), 0, 0, 10, 0xAA, 0xBB];
        let parsed = parse(&buf).expect("should parse legacy v1");
        assert_eq!(parsed.format, Format::PcmF32);
        assert_eq!(parsed.channel, 0);
        assert_eq!(parsed.seq, 10);
        assert_eq!(parsed.sample_rate, 48_000);
        assert_eq!(parsed.payload, &[0xAA, 0xBB]);
    }
}
