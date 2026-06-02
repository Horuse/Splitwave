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
mod wav;

#[cfg(target_os = "macos")]
pub use aac::AacRecorder;
pub use aiff::AiffRecorder;
pub use flac::FlacRecorder;
pub use mp3::Mp3Recorder;
pub use opus::OpusRecorder;
pub use wav::WavRecorder;

pub trait AudioEncoder: Send {
	fn write_stereo(&mut self, samples: &[f32]) -> AppResult<()>;
	fn flush(&mut self) -> AppResult<()>;
	fn finalize(self: Box<Self>) -> AppResult<()>;
}

pub fn build_encoder(
	path: &Path,
	sample_rate: u32,
	format: RecordingFormat,
) -> AppResult<Box<dyn AudioEncoder>> {
	match format {
		RecordingFormat::Wav { bit_depth } => {
			Ok(Box::new(WavRecorder::create(path, sample_rate, bit_depth)?))
		}
		RecordingFormat::Flac {
			bit_depth,
			compression,
		} => Ok(Box::new(FlacRecorder::create(
			path,
			sample_rate,
			bit_depth,
			compression,
		)?)),
		RecordingFormat::Opus {
			bitrate,
			application,
		} => Ok(Box::new(OpusRecorder::create(path, application, bitrate)?)),
		RecordingFormat::Mp3 { bitrate_kbps } => Ok(Box::new(Mp3Recorder::create(
			path,
			sample_rate,
			bitrate_kbps,
		)?)),
		RecordingFormat::Aac { bitrate } => {
			#[cfg(target_os = "macos")]
			{
				Ok(Box::new(AacRecorder::create(path, sample_rate, bitrate)?))
			}
			#[cfg(not(target_os = "macos"))]
			{
				let _ = (path, sample_rate, bitrate);
				Err(crate::error::AppError::Stream(
					"AAC recording is macOS-only".into(),
				))
			}
		}
		RecordingFormat::Aiff { bit_depth } => Ok(Box::new(AiffRecorder::create(
			path,
			sample_rate,
			bit_depth,
		)?)),
	}
}
