//! WAV PCM (16/24-bit int + 32-bit float) writer with crash-resistant headers.
//!
//! Each `flush` patches the RIFF / fact / data chunk sizes so the file on disk
//! is always a valid WAV at the last flush boundary.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
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
        let format = WavFormat::from(bit_depth);
        let file = File::create(path)
            .map_err(|e| AppError::Stream(format!("create {}: {e}", path.display())))?;
        let mut inner = BufWriter::new(file);
        write_header(&mut inner, sample_rate, channels, format, 0)
            .map_err(|e| AppError::Stream(format!("write wav header: {e}")))?;
        Ok(Self {
            inner,
            samples_per_channel: 0,
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
