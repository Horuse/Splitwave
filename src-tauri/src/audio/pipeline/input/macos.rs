use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::AppHandle;

use crate::audio::effects::MeterHandle;
use crate::audio::graph::{InputSpec, ValidInput};
use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

use super::{resolve_audio_file, start_audio_file, InputHandle, ResolvedInput};

/// The graph downstream is laid out from the format resolved before the capture
/// started. A tap follows the default output device, so switching it between
/// resolve and start shifts the rate; surface that instead of feeding the graph
/// mistimed audio.
const CAPTURE_CHANNELS: u32 = 2;

fn check_capture_format(capture: &crate::audio::capture::Capture) -> AppResult<()> {
    if capture.channels() != CAPTURE_CHANNELS {
        return Err(AppError::Stream(format!(
            "capture channel layout changed while starting: expected {CAPTURE_CHANNELS} ch, got {} ch",
            capture.channels()
        )));
    }
    Ok(())
}

pub(in crate::audio::pipeline) fn resolve_input(inp: &ValidInput) -> AppResult<ResolvedInput> {
    match &inp.spec {
        InputSpec::Microphone { device_id } => super::cpal_input::resolve_cpal_input(device_id),
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
        } => super::cpal_input::start_cpal_input_stream(
            node_id,
            device,
            config,
            sample_format,
            src_channels,
            bridge,
            meter,
            app,
        ),
        ResolvedInput::SystemAudio {
            sample_rate,
            exclude_current_app,
        } => {
            let capture = crate::audio::capture::Capture::start_system(
                exclude_current_app,
                sample_rate,
                bridge,
            )?;
            check_capture_format(&capture)?;
            Ok(InputHandle::Capture(capture))
        }
        ResolvedInput::AppAudio {
            sample_rate,
            bundle_id,
        } => {
            let capture =
                crate::audio::capture::Capture::start_app(&bundle_id, sample_rate, bridge)?;
            check_capture_format(&capture)?;
            Ok(InputHandle::Capture(capture))
        }
        ResolvedInput::AudioFile { path, .. } => {
            start_audio_file(node_id, path, bridge, paused, app)
        }
    }
}
