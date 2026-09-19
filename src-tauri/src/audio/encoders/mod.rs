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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::graph::{
        AiffBitDepth, FlacBitDepth, FlacCompression, OpusApplication, WavBitDepth,
    };

    fn temp(name: &str, extension: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{name}-{}.{extension}", std::process::id()))
    }

    fn sine(frames: usize, channels: u16) -> Vec<f32> {
        (0..frames * channels as usize)
            .map(|i| 0.5 * ((i / channels as usize) as f32 * 440.0 * 6.28 / 48_000.0).sin())
            .collect()
    }

    /// Writes a real file through each format's encoder and finalises it.
    /// WAV/AIFF get verified through `read_peaks`; compressed formats only
    /// assert that a non-empty file landed (their readers are decoder-stack
    /// specific and the peaks command refuses them by design).
    fn roundtrip(name: &str, format: RecordingFormat, channels: u16, check_peaks: bool) {
        let extension = match &format {
            RecordingFormat::Wav { .. } => "wav",
            RecordingFormat::Aiff { .. } => "aiff",
            RecordingFormat::Flac { .. } => "flac",
            RecordingFormat::Opus { .. } => "opus",
            RecordingFormat::Mp3 { .. } => "mp3",
            #[cfg(target_os = "macos")]
            RecordingFormat::Aac { .. } => "m4a",
        };
        let path = temp(name, extension);
        let mut enc = build_encoder(&path, 48_000, channels, format, false).expect("build");
        enc.write_interleaved(&sine(48_000, channels))
            .expect("write");
        enc.flush().expect("flush");
        // finalize consumes the encoder and closes the file.
        enc.finalize().expect("finalize");
        assert!(path.exists(), "encoder must leave a file");
        if check_peaks {
            let peaks = read_peaks(&path, 0, 1024, 16).expect("read peaks");
            assert_eq!(peaks.sample_rate, 48_000);
            assert_eq!(peaks.channels, channels as u32);
            assert!(peaks.total_frames >= 48_000 / 2, "frames recorded");
            let max_peak = peaks.maxs.iter().flatten().fold(0.0f32, |m, s| m.max(*s));
            assert!(max_peak > 0.2, "sine must be visible in peaks: {max_peak}");
            assert!(max_peak <= 0.6);
        } else {
            assert!(
                std::fs::metadata(&path)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false),
                "compressed formats must still write a non-empty file"
            );
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn wav_encoder_roundtrip_stereo() {
        roundtrip(
            "wav-st",
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32,
            },
            2,
            true,
        );
    }

    #[test]
    fn wav_encoder_roundtrip_i24() {
        roundtrip(
            "wav-i24",
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::I24,
            },
            2,
            true,
        );
    }

    #[test]
    fn aiff_encoder_roundtrip() {
        roundtrip(
            "aiff",
            RecordingFormat::Aiff {
                bit_depth: AiffBitDepth::I24,
            },
            2,
            true,
        );
    }

    #[test]
    fn flac_encoder_writes_nonempty_file() {
        roundtrip(
            "flac",
            RecordingFormat::Flac {
                bit_depth: FlacBitDepth::I24,
                compression: FlacCompression::Default,
            },
            2,
            false,
        );
    }

    #[test]
    fn opus_encoder_writes_nonempty_file() {
        roundtrip(
            "opus",
            RecordingFormat::Opus {
                bitrate: 96_000,
                application: OpusApplication::Audio,
            },
            2,
            false,
        );
    }

    #[test]
    fn mp3_encoder_writes_nonempty_file() {
        roundtrip("mp3", RecordingFormat::Mp3 { bitrate_kbps: 128 }, 2, false);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn aac_encoder_writes_nonempty_file() {
        roundtrip("aac", RecordingFormat::Aac { bitrate: 128_000 }, 2, false);
    }

    #[test]
    fn build_encoder_rejects_bad_channel_counts() {
        let path = temp("bad-ch", "opus");
        for channels in [0u16, 3u16] {
            let err = build_encoder(
                &path,
                48_000,
                channels,
                RecordingFormat::Opus {
                    bitrate: 96_000,
                    application: OpusApplication::Audio,
                },
                false,
            )
            .err()
            .expect("opus allows 1..2 channels");
            let crate::error::AppError::Validation(msg) = err else {
                panic!("validation expected");
            };
            assert!(msg.contains("channels"), "{msg}");
        }
        let err = build_encoder(
            &path,
            48_000,
            9,
            RecordingFormat::Flac {
                bit_depth: FlacBitDepth::I24,
                compression: FlacCompression::Default,
            },
            false,
        )
        .err()
        .expect("flac caps at 8 channels");
        let crate::error::AppError::Validation(msg) = err else {
            panic!("validation expected");
        };
        assert!(msg.contains("1..8"), "{msg}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn append_rejects_compressed_formats() {
        let err = validate_append_target(
            &temp("nope", "opus"),
            48_000,
            2,
            RecordingFormat::Opus {
                bitrate: 96_000,
                application: OpusApplication::Audio,
            },
        )
        .unwrap_err();
        let crate::error::AppError::Validation(msg) = err else {
            panic!("validation expected");
        };
        assert!(msg.contains("WAV/AIFF"), "{msg}");
    }

    #[test]
    fn wav_append_validates_against_mismatch() {
        let path = temp("append-wav", "wav");
        let mut enc = build_encoder(
            &path,
            48_000,
            2,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32,
            },
            false,
        )
        .expect("build");
        enc.write_interleaved(&sine(4800, 2)).expect("write");
        enc.finalize().expect("finalize");
        // Same shape → base frames reported.
        let base = validate_append_target(
            &path,
            48_000,
            2,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32,
            },
        )
        .expect("append valid");
        assert_eq!(base, 4800);
        // Mismatched shape → refused.
        assert!(validate_append_target(
            &path,
            44_100,
            2,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32
            },
        )
        .is_err());
        assert!(validate_append_target(
            &path,
            48_000,
            1,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32
            },
        )
        .is_err());
        assert!(validate_append_target(
            &path,
            48_000,
            2,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::I24
            },
        )
        .is_err());
        assert!(validate_append_target(
            &temp("missing-file", "wav"),
            48_000,
            2,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32
            },
        )
        .is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn aiff_append_validates() {
        let path = temp("append-aiff", "aiff");
        let mut enc = build_encoder(
            &path,
            48_000,
            2,
            RecordingFormat::Aiff {
                bit_depth: AiffBitDepth::I24,
            },
            false,
        )
        .expect("build");
        enc.write_interleaved(&sine(2400, 2)).expect("write");
        enc.finalize().expect("finalize");
        let base = validate_append_target(
            &path,
            48_000,
            2,
            RecordingFormat::Aiff {
                bit_depth: AiffBitDepth::I24,
            },
        )
        .expect("append valid");
        assert_eq!(base, 2400);
        std::fs::remove_file(&path).ok();
    }
}
