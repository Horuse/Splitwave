use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::info;

use crate::audio::device::{self, DeviceKind};
use crate::audio::graph::{InputSpec, ValidInput};
use crate::audio::input_bridge::BroadcastRx;
use crate::audio::streams;
use crate::error::AppResult;

use super::super::native::native_config;
use super::super::STATE_EVENT;
use super::{resolve_audio_file, start_audio_file, InputHandle, ResolvedInput, SCK_SR};

/// ScreenCaptureKit always delivers interleaved stereo by configuration.
const SCK_CHANNELS: usize = 2;

pub(in crate::audio::pipeline) fn resolve_input(inp: &ValidInput) -> AppResult<ResolvedInput> {
    match &inp.spec {
        InputSpec::Microphone { device_id } => {
            let device = device::find(DeviceKind::Input, device_id)?;
            let native = native_config(DeviceKind::Input, &device, device_id)?;
            Ok(ResolvedInput::Cpal {
                device,
                config: native.config,
                sample_format: native.sample_format,
                src_channels: native.channels as usize,
                sample_rate: native.sample_rate,
            })
        }
        InputSpec::SystemAudio { exclude_current_app } => Ok(ResolvedInput::SystemAudio {
            sample_rate: SCK_SR,
            exclude_current_app: *exclude_current_app,
        }),
        InputSpec::AppAudio { bundle_id } => Ok(ResolvedInput::AppAudio {
            sample_rate: SCK_SR,
            bundle_id: bundle_id.clone(),
        }),
        InputSpec::AudioFile { file_path } => resolve_audio_file(file_path),
    }
}

pub(in crate::audio::pipeline) fn start_input_stream(
    node_id: &str,
    resolved: ResolvedInput,
    bridge: BroadcastRx,
    paused: Option<Arc<AtomicBool>>,
    app: &AppHandle,
) -> AppResult<InputHandle> {
    match resolved {
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
        ResolvedInput::AudioFile { path, .. } => {
            start_audio_file(node_id, path, bridge, paused, app)
        }
    }
}
