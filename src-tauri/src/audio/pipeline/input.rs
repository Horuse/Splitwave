use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::AppHandle;
use tracing::info;
#[cfg(target_os = "macos")]
use {serde_json::json, tauri::Emitter};

#[cfg(target_os = "macos")]
use crate::audio::device::{self, DeviceKind};
use crate::audio::graph::{InputSpec, ValidInput};
use crate::audio::input_bridge::BroadcastRx;
#[cfg(target_os = "macos")]
use crate::audio::streams;
use crate::error::AppResult;

use super::file_reader::{probe_audio_file, start_audio_file_reader, AudioFileReader};
#[cfg(target_os = "macos")]
use super::STATE_EVENT;

/// ScreenCaptureKit always delivers interleaved stereo by configuration.
#[cfg(target_os = "macos")]
const SCK_CHANNELS: usize = 2;
/// ScreenCaptureKit sample rate request. 48 kHz is macOS's universal audio
/// rate and matches AVAudioSession / CoreAudio's preferred output, so no
/// resampling happens on the SCK delivery side.
const SCK_SR: u32 = 48_000;

/// RAII handle held only for its `Drop` -- stops the cpal stream, tears
/// down the SCStream, or signals + joins the file reader thread.
#[allow(dead_code)]
pub(super) enum InputHandle {
    Cpal(cpal::Stream),
    Capture(crate::audio::capture::Capture),
    AudioFile(AudioFileReader),
}

pub(super) enum ResolvedInput {
    #[cfg(target_os = "macos")]
    Cpal {
        device: cpal::Device,
        config: cpal::StreamConfig,
        sample_format: cpal::SampleFormat,
        src_channels: usize,
        sample_rate: u32,
    },
    #[cfg(not(target_os = "macos"))]
    PwSource {
        node_id: String,
        sample_rate: u32,
    },
    SystemAudio {
        sample_rate: u32,
        // PipeWire sink-monitor capture can't exclude our own output.
        #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
            #[cfg(target_os = "macos")]
            ResolvedInput::Cpal { sample_rate, .. } => *sample_rate,
            #[cfg(not(target_os = "macos"))]
            ResolvedInput::PwSource { sample_rate, .. } => *sample_rate,
            ResolvedInput::SystemAudio { sample_rate, .. } => *sample_rate,
            ResolvedInput::AppAudio { sample_rate, .. } => *sample_rate,
            ResolvedInput::AudioFile { sample_rate, .. } => *sample_rate,
        }
    }
}

pub(super) fn resolve_input(inp: &ValidInput) -> AppResult<ResolvedInput> {
    match &inp.spec {
        #[cfg(target_os = "macos")]
        InputSpec::Microphone { device_id } => {
            let device = device::find(DeviceKind::Input, device_id)?;
            let native = super::native_config(DeviceKind::Input, &device, device_id)?;
            Ok(ResolvedInput::Cpal {
                device,
                config: native.config,
                sample_format: native.sample_format,
                src_channels: native.channels as usize,
                sample_rate: native.sample_rate,
            })
        }
        #[cfg(not(target_os = "macos"))]
        InputSpec::Microphone { device_id } => Ok(ResolvedInput::PwSource {
            node_id: device_id.clone(),
            sample_rate: 48_000,
        }),
        InputSpec::SystemAudio {
            exclude_current_app,
        } => Ok(ResolvedInput::SystemAudio {
            sample_rate: SCK_SR,
            exclude_current_app: *exclude_current_app,
        }),
        InputSpec::AppAudio { bundle_id } => Ok(ResolvedInput::AppAudio {
            sample_rate: SCK_SR,
            bundle_id: bundle_id.clone(),
        }),
        InputSpec::AudioFile { file_path } => {
            let path = PathBuf::from(file_path);
            let info = probe_audio_file(&path)?;
            Ok(ResolvedInput::AudioFile {
                sample_rate: info.sample_rate,
                path,
            })
        }
    }
}

pub(super) fn start_input_stream(
    node_id: &str,
    resolved: ResolvedInput,
    bridge: BroadcastRx,
    paused: Option<Arc<AtomicBool>>,
    app: &AppHandle,
) -> AppResult<InputHandle> {
    match resolved {
        #[cfg(target_os = "macos")]
        ResolvedInput::Cpal {
            device,
            config,
            sample_format,
            src_channels,
            ..
        } => {
            let app_err = app.clone();
            let err_cb = move |e: cpal::StreamError| {
                let _ = app_err.emit(
                    STATE_EVENT,
                    json!({ "kind": "error", "message": format!("input: {e}") }),
                );
            };
            let stream = streams::build_input_stream(
                &device,
                &config,
                sample_format,
                src_channels,
                bridge,
                None,
                err_cb,
            )?;
            Ok(InputHandle::Cpal(stream))
        }
        #[cfg(not(target_os = "macos"))]
        ResolvedInput::PwSource { node_id, .. } => {
            let mut bridge = bridge;
            let cb = move |samples: &[f32]| {
                bridge.apply_commands();
                bridge.broadcast(samples);
            };
            let capture = if let Some(sink) = node_id.strip_prefix("monitor:") {
                info!(sink, "starting microphone capture (PipeWire sink monitor)");
                crate::audio::capture::Capture::start_sink_monitor(sink, cb)?
            } else {
                info!(%node_id, "starting microphone capture (PipeWire source)");
                crate::audio::capture::Capture::start_source(&node_id, cb)?
            };
            Ok(InputHandle::Capture(capture))
        }
        #[cfg(target_os = "macos")]
        ResolvedInput::SystemAudio {
            sample_rate,
            exclude_current_app,
        } => {
            info!(
                sample_rate,
                exclude_current_app, "starting system-audio capture (ScreenCaptureKit)"
            );
            let capture = crate::audio::capture::Capture::start_system(
                exclude_current_app,
                sample_rate,
                SCK_CHANNELS as u32,
                bridge,
            )?;
            Ok(InputHandle::Capture(capture))
        }
        #[cfg(target_os = "macos")]
        ResolvedInput::AppAudio {
            sample_rate,
            bundle_id,
        } => {
            info!(sample_rate, %bundle_id, "starting app-audio capture (ScreenCaptureKit)");
            let capture = crate::audio::capture::Capture::start_app(
                &bundle_id,
                sample_rate,
                SCK_CHANNELS as u32,
                bridge,
            )?;
            Ok(InputHandle::Capture(capture))
        }
        #[cfg(not(target_os = "macos"))]
        ResolvedInput::SystemAudio { .. } => {
            info!("starting system-audio capture (PipeWire sink monitor)");
            let mut bridge = bridge;
            let capture = crate::audio::capture::Capture::start_system(move |samples| {
                bridge.apply_commands();
                bridge.broadcast(samples);
            })?;
            Ok(InputHandle::Capture(capture))
        }
        #[cfg(not(target_os = "macos"))]
        ResolvedInput::AppAudio { bundle_id, .. } => {
            info!(%bundle_id, "starting app-audio capture (PipeWire tap)");
            let mut bridge = bridge;
            let capture = crate::audio::capture::Capture::start_app(&bundle_id, move |samples| {
                bridge.apply_commands();
                bridge.broadcast(samples);
            })?;
            Ok(InputHandle::Capture(capture))
        }
        ResolvedInput::AudioFile { path, .. } => {
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
    }
}
