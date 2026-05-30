use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::AppHandle;
use tracing::info;

use crate::audio::graph::{InputSpec, ValidInput};
use crate::audio::input_bridge::BroadcastRx;
use crate::error::AppResult;

use super::{resolve_audio_file, start_audio_file, InputHandle, ResolvedInput, SCK_SR};

pub(in crate::audio::pipeline) fn resolve_input(inp: &ValidInput) -> AppResult<ResolvedInput> {
    match &inp.spec {
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
        ResolvedInput::SystemAudio { .. } => {
            info!("starting system-audio capture (PipeWire sink monitor)");
            let mut bridge = bridge;
            let capture = crate::audio::capture::Capture::start_system(move |samples| {
                bridge.apply_commands();
                bridge.broadcast(samples);
            })?;
            Ok(InputHandle::Capture(capture))
        }
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
            start_audio_file(node_id, path, bridge, paused, app)
        }
    }
}
