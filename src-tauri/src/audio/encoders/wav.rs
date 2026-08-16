//! WAV PCM (16/24-bit int + 32-bit float) writer with crash-resistant headers.
//!
//! Each `flush` patches the RIFF / fact / data chunk sizes so the file on disk
//! is always a valid WAV at the last flush boundary.

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::dither::Xorshift;
use super::AudioEncoder;
use crate::audio::graph::WavBitDepth;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WavFormat {
    F32,
    I24,
    I16,
}

impl From<WavBitDepth> for WavFormat {
    fn from(bd: WavBitDepth) -> Self {
        match bd {
            WavBitDepth::F32 => WavFormat::F32,
            WavBitDepth::I24 => WavFormat::I24,
            WavBitDepth::I16 => WavFormat::I16,
        }
    }
}

impl WavFormat {
    fn bits(self) -> u16 {
        match self {
            WavFormat::F32 => 32,
            WavFormat::I24 => 24,
            WavFormat::I16 => 16,
        }
    }
    fn bytes_per_sample(self) -> u32 {
        (self.bits() / 8) as u32
    }
    fn format_tag(self) -> u16 {
        match self {
            WavFormat::F32 => 3, // WAVE_FORMAT_IEEE_FLOAT
            _ => 1,              // WAVE_FORMAT_PCM
        }
    }
    fn header_size(self) -> u64 {
        // IEEE_FLOAT adds a `cbSize` field + `fact` chunk (required for non-PCM).
        match self {
            WavFormat::F32 => 58,
            _ => 44,
        }
    }
    fn data_size_offset(self) -> u64 {
        match self {
            WavFormat::F32 => 54,
            _ => 40,
        }
    }
    fn fact_samples_offset(self) -> Option<u64> {
        match self {
            WavFormat::F32 => Some(46),
            _ => None,
        }
    }
}

const OFFSET_RIFF_SIZE: u64 = 4;

pub struct WavRecorder {
    inner: BufWriter<File>,
    samples_per_channel: u64,
    channels: u16,
    format: WavFormat,
    dither: Xorshift,
    frame: Vec<u8>,
}

impl WavRecorder {
    pub fn create(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        bit_depth: WavBitDepth,
    ) -> AppResult<Self> {
        Self::open(path, sample_rate, channels, bit_depth, false)
    }

    /// Opens an existing WAV and positions writes at the end of its data chunk,
    /// carrying the file's current sample count so `flush` patches the header
    /// with the cumulative total. Falls back to a fresh file when none exists.
    pub fn create_append(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        bit_depth: WavBitDepth,
    ) -> AppResult<Self> {
        Self::open(path, sample_rate, channels, bit_depth, true)
    }

    fn open(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        bit_depth: WavBitDepth,
        append: bool,
    ) -> AppResult<Self> {
        let format = WavFormat::from(bit_depth);
        let mut samples_per_channel: u64 = 0;
        let file = if append && path.exists() {
            let h = read_header(path)?;
            check_matches(&h, sample_rate, channels, format)?;
            let mut f = File::options()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|e| {
                    AppError::Stream(format!("open {} for append: {e}", path.display()))
                })?;
            f.seek(SeekFrom::Start(h.data_end))
                .map_err(|e| AppError::Stream(format!("seek wav data: {e}")))?;
            samples_per_channel = h.samples_per_channel;
            f
        } else {
            File::create(path)
                .map_err(|e| AppError::Stream(format!("create {}: {e}", path.display())))?
        };
        let mut inner = BufWriter::new(file);
        if !(append && path.exists()) {
            write_header(&mut inner, sample_rate, channels, format, 0)
                .map_err(|e| AppError::Stream(format!("write wav header: {e}")))?;
        }
        Ok(Self {
            inner,
            samples_per_channel,
            channels,
            format,
            dither: Xorshift::seed(0x9e3779b97f4a7c15),
            frame: vec![0; channels as usize * format.bytes_per_sample() as usize],
        })
    }

    fn write_pcm_int(
        &mut self,
        samples: &[f32],
        max: f32,
        min: f32,
        byte_count: usize,
    ) -> AppResult<()> {
        let n = self.channels as usize;
        for frame in samples.chunks_exact(n) {
            for (i, &s) in frame.iter().enumerate() {
                let dithered = s * max + self.dither.tpdf();
                let clamped = dithered.clamp(min, max);
                let q = clamped.round() as i32;
                let le = q.to_le_bytes();
                self.frame[i * byte_count..(i + 1) * byte_count].copy_from_slice(&le[..byte_count]);
            }
            self.inner
                .write_all(&self.frame[..byte_count * n])
                .map_err(|e| AppError::Stream(format!("write wav: {e}")))?;
        }
        Ok(())
    }

    fn write_f32(&mut self, samples: &[f32]) -> AppResult<()> {
        let n = self.channels as usize;
        for frame in samples.chunks_exact(n) {
            for (i, &s) in frame.iter().enumerate() {
                self.frame[i * 4..(i + 1) * 4].copy_from_slice(&s.to_le_bytes());
            }
            self.inner
                .write_all(&self.frame[..4 * n])
                .map_err(|e| AppError::Stream(format!("write wav: {e}")))?;
        }
        Ok(())
    }
}

impl AudioEncoder for WavRecorder {
    fn write_interleaved(&mut self, samples: &[f32]) -> AppResult<()> {
        match self.format {
            WavFormat::F32 => self.write_f32(samples)?,
            WavFormat::I24 => self.write_pcm_int(samples, 8_388_607.0, -8_388_608.0, 3)?,
            WavFormat::I16 => self.write_pcm_int(samples, 32_767.0, -32_768.0, 2)?,
        }
        self.samples_per_channel += (samples.len() / self.channels as usize) as u64;
        Ok(())
    }

    fn flush(&mut self) -> AppResult<()> {
        self.inner
            .flush()
            .map_err(|e| AppError::Stream(format!("flush wav: {e}")))?;
        let bps = self.format.bytes_per_sample() as u64;
        let data_size = self.samples_per_channel * (self.channels as u64) * bps;
        let header_size = self.format.header_size();
        let data_size_u32 = u32::try_from(data_size).unwrap_or(u32::MAX);
        let riff_size_u32 = data_size_u32.saturating_add((header_size - 8) as u32);
        let samples_u32 = u32::try_from(self.samples_per_channel).unwrap_or(u32::MAX);

        let file = self.inner.get_mut();
        file.seek(SeekFrom::Start(OFFSET_RIFF_SIZE))
            .map_err(|e| AppError::Stream(format!("seek wav: {e}")))?;
        file.write_all(&riff_size_u32.to_le_bytes())
            .map_err(|e| AppError::Stream(format!("patch riff size: {e}")))?;
        if let Some(off) = self.format.fact_samples_offset() {
            file.seek(SeekFrom::Start(off))
                .map_err(|e| AppError::Stream(format!("seek wav: {e}")))?;
            file.write_all(&samples_u32.to_le_bytes())
                .map_err(|e| AppError::Stream(format!("patch fact samples: {e}")))?;
        }
        file.seek(SeekFrom::Start(self.format.data_size_offset()))
            .map_err(|e| AppError::Stream(format!("seek wav: {e}")))?;
        file.write_all(&data_size_u32.to_le_bytes())
            .map_err(|e| AppError::Stream(format!("patch data size: {e}")))?;
        file.seek(SeekFrom::End(0))
            .map_err(|e| AppError::Stream(format!("seek wav end: {e}")))?;
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
    format: WavFormat,
    samples_per_channel: u32,
) -> std::io::Result<()> {
    let bps = format.bytes_per_sample();
    let block_align = channels * bps as u16;
    let byte_rate = sample_rate.saturating_mul((channels as u32) * bps);
    let data_size = samples_per_channel.saturating_mul((channels as u32) * bps);
    let riff_size = data_size.saturating_add((format.header_size() - 8) as u32);

    w.write_all(b"RIFF")?;
    w.write_all(&riff_size.to_le_bytes())?;
    w.write_all(b"WAVE")?;

    w.write_all(b"fmt ")?;
    let fmt_size: u32 = if format.fact_samples_offset().is_some() {
        18
    } else {
        16
    };
    w.write_all(&fmt_size.to_le_bytes())?;
    w.write_all(&format.format_tag().to_le_bytes())?;
    w.write_all(&channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&format.bits().to_le_bytes())?;

    if format.fact_samples_offset().is_some() {
        w.write_all(&0u16.to_le_bytes())?;
        w.write_all(b"fact")?;
        w.write_all(&4u32.to_le_bytes())?;
        w.write_all(&samples_per_channel.to_le_bytes())?;
    }

    w.write_all(b"data")?;
    w.write_all(&data_size.to_le_bytes())?;
    Ok(())
}

struct WavHeader {
    sample_rate: u32,
    channels: u16,
    format: WavFormat,
    data_end: u64,
    samples_per_channel: u64,
}

fn check_matches(
    h: &WavHeader,
    sample_rate: u32,
    channels: u16,
    format: WavFormat,
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
    if h.format != format {
        return Err(AppError::Validation(format!(
            "append mismatch: file is {}-bit but this recording is {}-bit",
            h.format.bits(),
            format.bits()
        )));
    }
    Ok(())
}

/// Validates an existing WAV's header against the requested parameters so a
/// mismatch surfaces synchronously at start rather than after the recorder
/// thread has opened the file. Returns the file's current sample count.
pub(crate) fn validate_append(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    bit_depth: WavBitDepth,
) -> AppResult<u64> {
    let h = read_header(path)?;
    check_matches(&h, sample_rate, channels, WavFormat::from(bit_depth))?;
    Ok(h.samples_per_channel)
}

fn wav_format_from_tag(tag: u16, bits: u16) -> AppResult<WavFormat> {
    match (tag, bits) {
        (1, 16) => Ok(WavFormat::I16),
        (1, 24) => Ok(WavFormat::I24),
        (3, 32) => Ok(WavFormat::F32),
        _ => Err(AppError::Validation(format!(
            "unsupported WAV format (tag {tag}, {bits} bits)"
        ))),
    }
}

fn read_header(path: &Path) -> AppResult<WavHeader> {
    let mut f =
        File::open(path).map_err(|e| AppError::Stream(format!("open {}: {e}", path.display())))?;
    let mut id = [0u8; 4];
    let mut u32buf = [0u8; 4];
    let mut u16buf = [0u8; 2];

    let r = |e: std::io::Error| AppError::Stream(format!("read wav header: {e}"));
    f.read_exact(&mut id).map_err(r)?;
    if &id != b"RIFF" {
        return Err(AppError::Validation(format!(
            "{} is not a WAV file",
            path.display()
        )));
    }
    f.read_exact(&mut u32buf).map_err(r)?; // riff size, unused
    f.read_exact(&mut id).map_err(r)?;
    if &id != b"WAVE" {
        return Err(AppError::Validation(format!(
            "{} is not a WAV file",
            path.display()
        )));
    }

    let mut format = None;
    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut data_end = 0u64;
    let mut data_size = 0u32;

    loop {
        let read = f.read(&mut id).map_err(r)?;
        if read == 0 {
            break;
        }
        if read != 4 {
            return Err(AppError::Stream("truncated WAV header".into()));
        }
        f.read_exact(&mut u32buf).map_err(r)?;
        let size = u32::from_le_bytes(u32buf);
        match &id {
            b"fmt " => {
                f.read_exact(&mut u16buf).map_err(r)?;
                let tag = u16::from_le_bytes(u16buf);
                f.read_exact(&mut u16buf).map_err(r)?;
                channels = u16::from_le_bytes(u16buf);
                f.read_exact(&mut u32buf).map_err(r)?;
                sample_rate = u32::from_le_bytes(u32buf);
                f.read_exact(&mut u32buf).map_err(r)?; // byte rate
                f.read_exact(&mut u16buf).map_err(r)?; // block align
                f.read_exact(&mut u16buf).map_err(r)?; // bits
                let bits = u16::from_le_bytes(u16buf);
                format = Some(wav_format_from_tag(tag, bits)?);
                if size > 16 {
                    f.seek(SeekFrom::Current((size - 16) as i64)).map_err(r)?;
                }
            }
            b"data" => {
                let start = f.stream_position().map_err(r)?;
                data_size = size;
                data_end = start + size as u64;
                break;
            }
            _ => {
                let skip = size as u64 + (size & 1) as u64;
                f.seek(SeekFrom::Current(skip as i64)).map_err(r)?;
            }
        }
    }

    let format = format
        .ok_or_else(|| AppError::Validation(format!("{} has no fmt chunk", path.display())))?;
    if channels == 0 || sample_rate == 0 || data_end == 0 {
        return Err(AppError::Validation(format!(
            "{} has an invalid or missing header/data chunk",
            path.display()
        )));
    }
    let bps = format.bytes_per_sample() as u64;
    let samples_per_channel = (data_size as u64) / ((channels as u64) * bps);
    Ok(WavHeader {
        sample_rate,
        channels,
        format,
        data_end,
        samples_per_channel,
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
        bit_depth: WavBitDepth,
        append: bool,
    ) -> Box<dyn AudioEncoder> {
        if append {
            Box::new(WavRecorder::create_append(path, sample_rate, channels, bit_depth).unwrap())
        } else {
            Box::new(WavRecorder::create(path, sample_rate, channels, bit_depth).unwrap())
        }
    }

    #[test]
    fn append_extends_existing_wav() {
        let path = temp_path("append.wav");
        let _ = std::fs::remove_file(&path);
        let block = vec![0.25f32; 2048]; // 1024 frames, stereo

        let mut first = encoder(&path, 48_000, 2, WavBitDepth::F32, false);
        first.write_interleaved(&block).unwrap();
        first.finalize().unwrap();

        let mut r = encoder(&path, 48_000, 2, WavBitDepth::F32, true);
        r.write_interleaved(&block).unwrap();
        r.finalize().unwrap();

        let h = read_header(&path).unwrap();
        assert_eq!(h.sample_rate, 48_000);
        assert_eq!(h.channels, 2);
        assert_eq!(h.format, WavFormat::F32);
        assert_eq!(h.samples_per_channel, 2048);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn append_mismatch_is_rejected() {
        let path = temp_path("mismatch.wav");
        let _ = std::fs::remove_file(&path);
        let block = vec![0.0f32; 1024]; // 512 frames, mono
        let mut first = encoder(&path, 48_000, 1, WavBitDepth::I16, false);
        first.write_interleaved(&block).unwrap();
        first.finalize().unwrap();

        assert!(WavRecorder::create_append(&path, 44_100, 1, WavBitDepth::I16).is_err());
        assert!(WavRecorder::create_append(&path, 48_000, 2, WavBitDepth::I16).is_err());
        assert!(WavRecorder::create_append(&path, 48_000, 1, WavBitDepth::F32).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
