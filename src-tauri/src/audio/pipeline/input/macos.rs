use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::error;

use crate::audio::device::{self, DeviceKind};
use crate::audio::effects::MeterHandle;
use crate::audio::graph::{InputSpec, ValidInput};
use crate::audio::health;
use crate::audio::input_bridge::BroadcastRx;
use crate::audio::streams;
use crate::error::{AppError, AppResult};

use super::super::native::native_config;
use super::{resolve_audio_file, start_audio_file, InputHandle, ResolvedInput};

/// The graph downstream is laid out from the format resolved before the capture
/// started. A tap follows the default output device, so switching it between
/// resolve and start shifts the rate; surface that instead of feeding the graph
/// mistimed audio.
const CAPTURE_CHANNELS: u32 = 2;

fn check_capture_format(
    capture: &crate::audio::capture::Capture,
    expected_rate: u32,
) -> AppResult<()> {
    if capture.sample_rate() != expected_rate || capture.channels() != CAPTURE_CHANNELS {
        return Err(AppError::Stream(format!(
            "capture format changed while starting: expected {expected_rate} Hz / {CAPTURE_CHANNELS} ch, got {} Hz / {} ch",
            capture.sample_rate(),
            capture.channels()
        )));
    }
    Ok(())
}

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
        InputSpec::SystemAudio {
            exclude_current_app,
        } => Ok(ResolvedInput::SystemAudio {
            sample_rate: crate::audio::capture::capture_rate(),
            exclude_current_app: *exclude_current_app,
        }),
        InputSpec::AppAudio { bundle_id } => Ok(ResolvedInput::AppAudio {
            sample_rate: crate::audio::capture::capture_rate(),
            bundle_id: bundle_id.clone(),
        }),
        InputSpec::AudioFile { file_path } => resolve_audio_file(file_path),
        // Resolved as a network producer in build_output_graph, never here.
        InputSpec::NetReceiver { .. } | InputSpec::WebRtcRecv { .. } => {
            unreachable!("network inputs have no capture device")
        }
    }
}

pub(in crate::audio::pipeline) fn start_input_stream(
    node_id: &str,
    resolved: ResolvedInput,
    bridge: BroadcastRx,
    paused: Option<Arc<AtomicBool>>,
    meter: Option<MeterHandle>,
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
            let dead = Arc::new(AtomicBool::new(false));
            let dead_cb = dead.clone();
            let app_err = app.clone();
            let node_id_cb = node_id.to_string();
            let err_cb = move |e: cpal::StreamError| {
                if dead_cb.swap(true, Ordering::Relaxed) {
                    return;
                }
                health::bump(&health::STREAM_ERRORS, 1);
                error!(node_id = %node_id_cb, error = %e, "input stream error");
                let _ = app_err.emit(
                    "audio://input_error",
                    json!({ "nodeId": node_id_cb, "error": format!("{e}") }),
                );
            };
            let stream = streams::build_input_stream(
                &device,
                &config,
                sample_format,
                src_channels,
                bridge,
                meter,
                err_cb,
            )?;
            Ok(InputHandle::Cpal(stream))
        }
        ResolvedInput::SystemAudio {
            sample_rate,
            exclude_current_app,
        } => {
            let capture = crate::audio::capture::Capture::start_system(
                exclude_current_app,
                sample_rate,
                bridge,
            )?;
            check_capture_format(&capture, sample_rate)?;
            Ok(InputHandle::Capture(capture))
        }
        ResolvedInput::AppAudio {
            sample_rate,
            bundle_id,
        } => {
            let capture =
                crate::audio::capture::Capture::start_app(&bundle_id, sample_rate, bridge)?;
            check_capture_format(&capture, sample_rate)?;
            Ok(InputHandle::Capture(capture))
        }
        ResolvedInput::AudioFile { path, .. } => {
            start_audio_file(node_id, path, bridge, paused, app)
        }
    }
}
