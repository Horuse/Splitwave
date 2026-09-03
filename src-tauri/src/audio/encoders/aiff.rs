//! AIFF (Apple's RIFF cousin): big-endian PCM int (16/24-bit). Each `flush`
//! patches FORM/COMM/SSND size + frame count → file on disk is a valid AIFF
//! at the last flush boundary.

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::dither::Xorshift;
use super::AudioEncoder;
use crate::audio::graph::AiffBitDepth;
use crate::error::{AppError, AppResult};

const HEADER_SIZE: u64 = 54;
const OFFSET_FORM_SIZE: u64 = 4;
const OFFSET_NUM_FRAMES: u64 = 22;
const OFFSET_SSND_SIZE: u64 = 42;

pub struct AiffRecorder {
    inner: BufWriter<File>,
    samples_per_channel: u64,
    channels: u16,
    bit_depth: AiffBitDepth,
    dither: Xorshift,
    frame: Vec<u8>,
}

impl AiffRecorder {
    pub fn create(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        bit_depth: AiffBitDepth,
    ) -> AppResult<Self> {
        Self::open(path, sample_rate, channels, bit_depth, false)
    }

    /// Opens an existing AIFF and positions writes at the end of its SSND chunk,
    /// carrying the file's current frame count so `flush` patches the header
    /// with the cumulative total. Falls back to a fresh file when none exists.
    pub fn create_append(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        bit_depth: AiffBitDepth,
    ) -> AppResult<Self> {
        Self::open(path, sample_rate, channels, bit_depth, true)
    }

    fn open(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        bit_depth: AiffBitDepth,
        append: bool,
    ) -> AppResult<Self> {
        let mut samples_per_channel: u64 = 0;
        let file = if append && path.exists() {
            let h = read_header(path)?;
            check_matches(&h, sample_rate, channels, bit_depth)?;
            let mut f = File::options()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| {
                    AppError::Stream(format!("open {} for append: {e}", path.display()))
                })?;
            f.seek(SeekFrom::Start(h.data_end))
                .map_err(|e| AppError::Stream(format!("seek aiff data: {e}")))?;
            samples_per_channel = h.samples_per_channel;
            f
        } else {
            File::create(path)
                .map_err(|e| AppError::Stream(format!("create {}: {e}", path.display())))?
        };
        let mut inner = BufWriter::new(file);
        if !(append && path.exists()) {
            write_header(&mut inner, sample_rate, channels, bit_depth, 0)
                .map_err(|e| AppError::Stream(format!("write aiff header: {e}")))?;
        }
        let bps = match bit_depth {
            AiffBitDepth::I16 => 2,
            AiffBitDepth::I24 => 3,
        };
        Ok(Self {
            inner,
            samples_per_channel,
            channels,
            bit_depth,
            dither: Xorshift::seed(0x9e3779b97f4a7c15),
            frame: vec![0; channels as usize * bps],
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
        let n = self.channels as usize;
        for frame in samples.chunks_exact(n) {
            for (i, &s) in frame.iter().enumerate() {
                let dithered = s * max + self.dither.tpdf();
                let q = dithered.clamp(min, max).round() as i32;
                let be = q.to_be_bytes();
                // `i32::to_be_bytes` = [MSB, ..., LSB]; i16 needs the last 2, i24 the last 3.
                self.frame[i * bps..(i + 1) * bps].copy_from_slice(&be[4 - bps..4]);
            }
            self.inner
                .write_all(&self.frame[..bps * n])
                .map_err(|e| AppError::Stream(format!("write aiff: {e}")))?;
        }
        Ok(())
    }
}

impl AudioEncoder for AiffRecorder {
    fn write_interleaved(&mut self, samples: &[f32]) -> AppResult<()> {
        self.write_pcm(samples)?;
        self.samples_per_channel += (samples.len() / self.channels as usize) as u64;
        Ok(())
    }

    fn flush(&mut self) -> AppResult<()> {
        self.inner
            .flush()
            .map_err(|e| AppError::Stream(format!("flush aiff: {e}")))?;

        let bps = self.bytes_per_sample() as u64;
        let data_size = self.samples_per_channel * (self.channels as u64) * bps;
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
    channels: u16,
    bit_depth: AiffBitDepth,
    num_frames: u32,
) -> std::io::Result<()> {
    let bits: u16 = match bit_depth {
        AiffBitDepth::I16 => 16,
        AiffBitDepth::I24 => 24,
    };
    let bps = (bits / 8) as u32;
    let data_size = num_frames.saturating_mul((channels as u32) * bps);
    let form_size = (HEADER_SIZE as u32 - 8).saturating_add(data_size);
    let ssnd_size = 8u32.saturating_add(data_size);

    w.write_all(b"FORM")?;
    w.write_all(&form_size.to_be_bytes())?;
    w.write_all(b"AIFF")?;

    w.write_all(b"COMM")?;
    w.write_all(&18u32.to_be_bytes())?;
    w.write_all(&(channels as i16).to_be_bytes())?;
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

struct AiffHeader {
    sample_rate: u32,
    channels: u16,
    bit_depth: AiffBitDepth,
    data_end: u64,
    samples_per_channel: u64,
}

fn check_matches(
    h: &AiffHeader,
    sample_rate: u32,
    channels: u16,
    bit_depth: AiffBitDepth,
) -> AppResult<()> {
    if h.sample_rate != sample_rate {
        return Err(AppError::Validation(format!(
            "append mismatch: file is {} Hz but this recording is {sample_rate} Hz",
            h.sample_rate
        )));
    }
    if h.channels != channels {
        return Err(AppError::Validation(format!(
            "append mismatch: file has {} channels but this recording uses {channels}",
            h.channels
        )));
    }
    if h.bit_depth != bit_depth {
        let bits = |bd: AiffBitDepth| if bd == AiffBitDepth::I16 { 16 } else { 24 };
        return Err(AppError::Validation(format!(
            "append mismatch: file is {}-bit but this recording is {}-bit",
            bits(h.bit_depth),
            bits(bit_depth)
        )));
    }
    Ok(())
}

pub(crate) fn validate_append(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    bit_depth: AiffBitDepth,
) -> AppResult<u64> {
    let h = read_header(path)?;
    check_matches(&h, sample_rate, channels, bit_depth)?;
    Ok(h.samples_per_channel)
}

/// Inverse of `sample_rate_to_extended_80`, for whole-number rates.
fn extended80_to_u32(bytes: [u8; 10]) -> u32 {
    let exp = u16::from_be_bytes([bytes[0], bytes[1]]);
    let mantissa = u64::from_be_bytes([
        bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9],
    ]);
    let shift = 63i32 + 16383i32 - exp as i32;
    if shift < 0 || shift >= 64 {
        return 0;
    }
    (mantissa >> shift as u32) as u32
}

fn read_header(path: &Path) -> AppResult<AiffHeader> {
    let mut f =
        File::open(path).map_err(|e| AppError::Stream(format!("open {}: {e}", path.display())))?;
    let mut id = [0u8; 4];
    let mut u32buf = [0u8; 4];
    let mut s16buf = [0u8; 2];

    let r = |e: std::io::Error| AppError::Stream(format!("read aiff header: {e}"));
    f.read_exact(&mut id).map_err(r)?;
    if &id != b"FORM" {
        return Err(AppError::Validation(format!(
            "{} is not an AIFF file",
            path.display()
        )));
    }
    f.read_exact(&mut u32buf).map_err(r)?; // form size, unused
    f.read_exact(&mut id).map_err(r)?;
    if &id != b"AIFF" {
        return Err(AppError::Validation(format!(
            "{} is not an AIFF file",
            path.display()
        )));
    }

    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bit_depth = None;
    let mut data_end = 0u64;
    let mut frames = 0u32;

    loop {
        let read = f.read(&mut id).map_err(r)?;
        if read == 0 {
            break;
        }
        if read != 4 {
            return Err(AppError::Stream("truncated AIFF header".into()));
        }
        f.read_exact(&mut u32buf).map_err(r)?;
        let size = u32::from_be_bytes(u32buf);
        match &id {
            b"COMM" => {
                f.read_exact(&mut s16buf).map_err(r)?;
                channels = i16::from_be_bytes(s16buf) as u16;
                f.read_exact(&mut u32buf).map_err(r)?;
                frames = u32::from_be_bytes(u32buf);
                f.read_exact(&mut s16buf).map_err(r)?;
                bit_depth = Some(match i16::from_be_bytes(s16buf) {
                    16 => AiffBitDepth::I16,
                    24 => AiffBitDepth::I24,
                    b => {
                        return Err(AppError::Validation(format!(
                            "unsupported AIFF bit depth {b}"
                        )))
                    }
                });
                let mut ext = [0u8; 10];
                f.read_exact(&mut ext).map_err(r)?;
                sample_rate = extended80_to_u32(ext);
            }
            b"SSND" => {
                f.read_exact(&mut u32buf).map_err(r)?; // offset
                let offset = u32::from_be_bytes(u32buf);
                f.read_exact(&mut u32buf).map_err(r)?; // block size
                let data_size = size.saturating_sub(8 + offset);
                data_end = f.stream_position().map_err(r)? + offset as u64 + data_size as u64;
                break;
            }
            _ => {
                let skip = size as u64 + (size & 1) as u64;
                f.seek(SeekFrom::Current(skip as i64)).map_err(r)?;
            }
        }
    }

    let bit_depth = bit_depth
        .ok_or_else(|| AppError::Validation(format!("{} has no COMM chunk", path.display())))?;
    if channels == 0 || sample_rate == 0 || data_end == 0 {
        return Err(AppError::Validation(format!(
            "{} has an invalid or missing COMM/SSND chunk",
            path.display()
        )));
    }
    Ok(AiffHeader {
        sample_rate,
        channels,
        bit_depth,
        data_end,
        samples_per_channel: frames as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("splitwave_test_{}_{}", std::process::id(), name));
        p
    }

    fn encoder(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        bit_depth: AiffBitDepth,
        append: bool,
    ) -> Box<dyn AudioEncoder> {
        if append {
            Box::new(AiffRecorder::create_append(path, sample_rate, channels, bit_depth).unwrap())
        } else {
            Box::new(AiffRecorder::create(path, sample_rate, channels, bit_depth).unwrap())
        }
    }

    #[test]
    fn append_extends_existing_aiff() {
        let path = temp_path("append.aiff");
        let _ = std::fs::remove_file(&path);
        let block = vec![0.25f32; 2048]; // 1024 frames, stereo

        let mut first = encoder(&path, 48_000, 2, AiffBitDepth::I16, false);
        first.write_interleaved(&block).unwrap();
        first.finalize().unwrap();

        let mut r = encoder(&path, 48_000, 2, AiffBitDepth::I16, true);
        r.write_interleaved(&block).unwrap();
        r.finalize().unwrap();

        let h = read_header(&path).unwrap();
        assert_eq!(h.sample_rate, 48_000);
        assert_eq!(h.channels, 2);
        assert_eq!(h.bit_depth, AiffBitDepth::I16);
        assert_eq!(h.samples_per_channel, 2048);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn append_mismatch_is_rejected() {
        let path = temp_path("mismatch.aiff");
        let _ = std::fs::remove_file(&path);
        let block = vec![0.0f32; 1024]; // 512 frames, mono
        let mut first = encoder(&path, 48_000, 1, AiffBitDepth::I16, false);
        first.write_interleaved(&block).unwrap();
        first.finalize().unwrap();

        assert!(AiffRecorder::create_append(&path, 44_100, 1, AiffBitDepth::I16).is_err());
        assert!(AiffRecorder::create_append(&path, 48_000, 2, AiffBitDepth::I16).is_err());
        assert!(AiffRecorder::create_append(&path, 48_000, 1, AiffBitDepth::I24).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
