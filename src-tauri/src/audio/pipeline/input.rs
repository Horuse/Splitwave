use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::info;

use crate::audio::device::{self, DeviceKind};
use crate::audio::effects::MeterHandle;
use crate::audio::graph::{InputSpec, ValidInput};
use crate::audio::input_bridge::BroadcastRx;
use crate::audio::streams;
#[cfg(not(target_os = "macos"))]
use crate::error::AppError;
use crate::error::AppResult;

use super::STATE_EVENT;

/// ScreenCaptureKit always delivers interleaved stereo by configuration.
const SCK_CHANNELS: usize = 2;
/// ScreenCaptureKit sample rate request. 48 kHz is macOS's universal audio
/// rate and matches AVAudioSession / CoreAudio's preferred output, so no
/// resampling happens on the SCK delivery side.
const SCK_SR: u32 = 48_000;

/// Unified RAII handle for the different input source backends. The wrapped
/// value is held only for its `Drop` side-effect: the cpal stream stops on
/// drop, the SCK capture tears down the SCStream on drop.
#[allow(dead_code)]
pub(super) enum InputHandle {
    Cpal(cpal::Stream),
    #[cfg(target_os = "macos")]
    Sck(crate::audio::sck_capture::SckCapture),
}

pub(super) enum ResolvedInput {
    Cpal {
        device: cpal::Device,
        config: cpal::StreamConfig,
        sample_format: cpal::SampleFormat,
        src_channels: usize,
        sample_rate: u32,
    },
    SystemAudio {
        sample_rate: u32,
        exclude_current_app: bool,
    },
    AppAudio {
        sample_rate: u32,
        bundle_id: String,
    },
}

impl ResolvedInput {
    pub(super) fn sample_rate(&self) -> u32 {
        match self {
            ResolvedInput::Cpal { sample_rate, .. } => *sample_rate,
            ResolvedInput::SystemAudio { sample_rate, .. } => *sample_rate,
            ResolvedInput::AppAudio { sample_rate, .. } => *sample_rate,
        }
    }
}

pub(super) fn resolve_input(inp: &ValidInput) -> AppResult<ResolvedInput> {
    match &inp.spec {
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
    }
}

pub(super) fn start_input_stream(
    resolved: ResolvedInput,
    bridge: BroadcastRx,
    meter: MeterHandle,
    app: &AppHandle,
) -> AppResult<InputHandle> {
    let app_err = app.clone();
    let err_cb = move |e: cpal::StreamError| {
        let _ = app_err.emit(
            STATE_EVENT,
            json!({ "kind": "error", "message": format!("input: {e}") }),
        );
    };

    match resolved {
        ResolvedInput::Cpal {
            device,
            config,
            sample_format,
            src_channels,
            ..
        } => {
            let stream = streams::build_input_stream(
                &device,
                &config,
                sample_format,
                src_channels,
                bridge,
                Some(meter),
                err_cb,
            )?;
            Ok(InputHandle::Cpal(stream))
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
            let capture = crate::audio::sck_capture::SckCapture::start_system(
                exclude_current_app,
                sample_rate,
                SCK_CHANNELS as u32,
                bridge,
                Some(meter),
            )?;
            Ok(InputHandle::Sck(capture))
        }
        #[cfg(target_os = "macos")]
        ResolvedInput::AppAudio {
            sample_rate,
            bundle_id,
        } => {
            info!(sample_rate, %bundle_id, "starting app-audio capture (ScreenCaptureKit)");
            let capture = crate::audio::sck_capture::SckCapture::start_app(
                &bundle_id,
                sample_rate,
                SCK_CHANNELS as u32,
                bridge,
                Some(meter),
            )?;
            Ok(InputHandle::Sck(capture))
        }
        #[cfg(not(target_os = "macos"))]
        ResolvedInput::SystemAudio { .. } | ResolvedInput::AppAudio { .. } => {
            drop(bridge);
            let _ = meter;
            Err(AppError::Stream(
                "System/App Audio capture is only supported on macOS".into(),
            ))
        }
    }
}
