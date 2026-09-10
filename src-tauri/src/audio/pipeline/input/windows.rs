use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::AppHandle;
use tracing::info;

use crate::audio::graph::{InputSpec, ValidInput};
use crate::audio::input_bridge::BroadcastRx;
use crate::error::AppResult;

use super::{resolve_audio_file, start_audio_file, InputHandle, ResolvedInput};

const LOOPBACK_FALLBACK_RATE: u32 = 48_000;

const LOOPBACK_CHANNELS: usize = 2;

pub(in crate::audio::pipeline) fn resolve_input(inp: &ValidInput) -> AppResult<ResolvedInput> {
    match &inp.spec {
        InputSpec::Microphone { device_id } => super::cpal::resolve_cpal_input(device_id),
        InputSpec::SystemAudio {
            exclude_current_app,
        } => Ok(ResolvedInput::SystemAudio {
            sample_rate: crate::audio::capture::loopback_mix_rate()
                .unwrap_or(LOOPBACK_FALLBACK_RATE),
            exclude_current_app: *exclude_current_app,
        }),
        InputSpec::AppAudio { bundle_id } => Ok(ResolvedInput::AppAudio {
            sample_rate: LOOPBACK_FALLBACK_RATE,
            bundle_id: bundle_id.clone(),
        }),
        InputSpec::AudioFile { file_path } => resolve_audio_file(file_path),
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
    meter: Option<crate::audio::effects::MeterHandle>,
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
            info!(
                sample_rate,
                exclude_current_app, "starting system-audio capture (WASAPI loopback)"
            );
            let capture = crate::audio::capture::Capture::start_system(
                exclude_current_app,
                sample_rate,
                LOOPBACK_CHANNELS as u32,
                bridge,
            )?;
            Ok(InputHandle::Capture(capture))
        }
        ResolvedInput::AppAudio {
            sample_rate,
            bundle_id,
        } => {
            info!(sample_rate, %bundle_id, "starting app-audio capture (WASAPI process loopback)");
            let capture = crate::audio::capture::Capture::start_app(
                &bundle_id,
                sample_rate,
                LOOPBACK_CHANNELS as u32,
                bridge,
            )?;
            Ok(InputHandle::Capture(capture))
        }
        ResolvedInput::AudioFile { path, .. } => {
            start_audio_file(node_id, path, bridge, paused, app)
        }
    }
}
