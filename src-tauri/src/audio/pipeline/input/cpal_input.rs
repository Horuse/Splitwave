use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::error;

use crate::audio::device::{self, DeviceKind};
use crate::audio::effects::MeterHandle;
use crate::audio::health;
use crate::audio::input_bridge::BroadcastRx;
use crate::audio::pipeline::native::native_config;
use crate::audio::streams;
use crate::error::AppResult;

use super::{InputHandle, ResolvedInput};

pub(super) fn resolve_cpal_input(device_id: &str) -> AppResult<ResolvedInput> {
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

pub(super) fn start_cpal_input_stream(
    node_id: &str,
    device: cpal::Device,
    config: cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    src_channels: usize,
    bridge: BroadcastRx,
    meter: Option<MeterHandle>,
    app: &AppHandle,
) -> AppResult<InputHandle> {
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
