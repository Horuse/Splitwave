use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::AppHandle;

use crate::audio::input_bridge::BroadcastRx;
use crate::error::AppResult;

use super::file_reader::{probe_audio_file, start_audio_file_reader, AudioFileReader};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

pub(super) use platform::{resolve_input, start_input_stream};

/// ScreenCaptureKit (macOS) and PipeWire (Linux) both deliver 48 kHz, matching
/// the device side so no resampling happens on capture delivery.
pub(super) const SCK_SR: u32 = 48_000;

/// RAII handle held only for its `Drop` -- stops the cpal stream, tears
/// down the capture, or signals + joins the file reader thread.
#[allow(dead_code)]
pub(super) enum InputHandle {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Cpal(cpal::Stream),
    Capture(crate::audio::capture::Capture),
    AudioFile(AudioFileReader),
}

pub(super) enum ResolvedInput {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Cpal {
        device: cpal::Device,
        config: cpal::StreamConfig,
        sample_format: cpal::SampleFormat,
        src_channels: usize,
        sample_rate: u32,
    },
    #[cfg(target_os = "linux")]
    PwSource {
        node_id: String,
        sample_rate: u32,
    },
    SystemAudio {
        sample_rate: u32,
        // PipeWire sink-monitor capture can't exclude our own output.
        #[cfg_attr(target_os = "linux", allow(dead_code))]
        exclude_current_app: bool,
    },
    AppAudio {
        sample_rate: u32,
        bundle_id: String,
    },
    AudioFile {
        sample_rate: u32,
        path: PathBuf,
    },
}

impl ResolvedInput {
    pub(super) fn sample_rate(&self) -> u32 {
        match self {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            ResolvedInput::Cpal { sample_rate, .. } => *sample_rate,
            #[cfg(target_os = "linux")]
            ResolvedInput::PwSource { sample_rate, .. } => *sample_rate,
            ResolvedInput::SystemAudio { sample_rate, .. } => *sample_rate,
            ResolvedInput::AppAudio { sample_rate, .. } => *sample_rate,
            ResolvedInput::AudioFile { sample_rate, .. } => *sample_rate,
        }
    }
}

/// Shared file probe -- both platforms resolve audio files identically.
pub(super) fn resolve_audio_file(file_path: &str) -> AppResult<ResolvedInput> {
    let path = PathBuf::from(file_path);
    let info = probe_audio_file(&path)?;
    Ok(ResolvedInput::AudioFile {
        sample_rate: info.sample_rate,
        path,
    })
}

/// Shared file-reader start -- both platforms drive audio files identically.
pub(super) fn start_audio_file(
    node_id: &str,
    path: PathBuf,
    bridge: BroadcastRx,
    paused: Option<Arc<AtomicBool>>,
    app: &AppHandle,
) -> AppResult<InputHandle> {
    // Loop is a runtime atomic, not in InputSpec; frontend syncs it
    // via `set_audio_file_loop` after pipeline start.
    let paused_arc = paused.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let reader = start_audio_file_reader(
        node_id.to_string(),
        path,
        bridge,
        false,
        paused_arc,
        app.clone(),
    )?;
    Ok(InputHandle::AudioFile(reader))
}
