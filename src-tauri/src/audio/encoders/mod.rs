//! Audio encoders for the `FileRecording` output node.

use std::path::Path;

use crate::audio::graph::RecordingFormat;
use crate::error::AppResult;

#[cfg(target_os = "macos")]
mod aac;
mod aiff;
mod dither;
mod flac;
mod mp3;
mod opus;
mod peaks;
mod wav;

#[cfg(target_os = "macos")]
pub use aac::AacRecorder;
pub use aiff::AiffRecorder;
pub use flac::FlacRecorder;
pub use mp3::Mp3Recorder;
pub use opus::OpusRecorder;
pub use peaks::{read_peaks, FilePeaks};
pub use wav::WavRecorder;

pub trait AudioEncoder: Send {
    fn write_interleaved(&mut self, samples: &[f32]) -> AppResult<()>;
    fn flush(&mut self) -> AppResult<()>;
    fn finalize(self: Box<Self>) -> AppResult<()>;
}

pub fn build_encoder(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    format: RecordingFormat,
    append: bool,
) -> AppResult<Box<dyn AudioEncoder>> {
    let max = format.max_channels();
    if channels == 0 || channels > max {
        return Err(crate::error::AppError::Validation(format!(
            "{channels} channels requested; this format allows 1..{max}"
        )));
    }
    match format {
        RecordingFormat::Wav { bit_depth } => {
            let rec = if append {
                WavRecorder::create_append(path, sample_rate, channels, bit_depth)?
            } else {
                WavRecorder::create(path, sample_rate, channels, bit_depth)?
            };
            Ok(Box::new(rec))
        }
        RecordingFormat::Flac {
            bit_depth,
            compression,
        } => Ok(Box::new(FlacRecorder::create(
            path,
            sample_rate,
            channels,
            bit_depth,
            compression,
        )?)),
        RecordingFormat::Opus {
            bitrate,
            application,
        } => Ok(Box::new(OpusRecorder::create(
            path,
            channels,
            application,
            bitrate,
        )?)),
        RecordingFormat::Mp3 { bitrate_kbps } => Ok(Box::new(Mp3Recorder::create(
            path,
            sample_rate,
            channels,
            bitrate_kbps,
        )?)),
        RecordingFormat::Aac { bitrate } => {
            #[cfg(target_os = "macos")]
            {
                Ok(Box::new(AacRecorder::create(
                    path,
                    sample_rate,
                    channels,
                    bitrate,
                )?))
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (path, sample_rate, channels, bitrate, append);
                Err(crate::error::AppError::Stream(
                    "AAC recording is macOS-only".into(),
                ))
            }
        }
        RecordingFormat::Aiff { bit_depth } => {
            let rec = if append {
                AiffRecorder::create_append(path, sample_rate, channels, bit_depth)?
            } else {
                AiffRecorder::create(path, sample_rate, channels, bit_depth)?
            };
            Ok(Box::new(rec))
        }
    }
}

/// Early, synchronous validation of an append target: reads the existing WAV or
/// AIFF header and checks it against the resolved sample rate, channel count and
/// bit depth. Compressed formats are rejected outright. Returns the file's
/// current per-channel sample count, which the recorder adds to its counters so
/// duration/size readouts start from the existing content, not zero.
pub(crate) fn validate_append_target(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    format: RecordingFormat,
) -> AppResult<u64> {
    match format {
        RecordingFormat::Wav { bit_depth } => {
            wav::validate_append(path, sample_rate, channels, bit_depth)
        }
        RecordingFormat::Aiff { bit_depth } => {
            aiff::validate_append(path, sample_rate, channels, bit_depth)
        }
        _ => Err(crate::error::AppError::Validation(
            "append is only supported for WAV/AIFF".into(),
        )),
    }
}
