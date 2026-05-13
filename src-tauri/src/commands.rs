use std::sync::mpsc;

use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use crate::audio::device::{self, DeviceInfo, DeviceKind, NativeDeviceInfo};
use crate::audio::engine::Command;
use crate::audio::graph::GraphSpec;
use crate::audio::permission::{self, PermissionState};
use crate::audio::system_audio::{self, AudioApplication};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const STATE_EVENT: &str = "audio://state";

#[tauri::command]
pub fn list_input_devices() -> AppResult<Vec<DeviceInfo>> {
    device::list_inputs()
}

#[tauri::command]
pub fn list_output_devices() -> AppResult<Vec<DeviceInfo>> {
    device::list_outputs()
}

#[tauri::command]
pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    system_audio::list_audio_applications()
}

#[tauri::command]
pub fn device_info(kind: DeviceKind, name: String) -> AppResult<NativeDeviceInfo> {
    device::device_info(kind, &name)
}

#[tauri::command]
pub fn check_screen_recording_permission() -> PermissionState {
    permission::screen_recording()
}

#[tauri::command]
pub fn start_pipeline(
    graph: GraphSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    let valid = graph.validate()?;
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::Start {
            graph: valid,
            app: app.clone(),
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    let result = reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?;
    if result.is_ok() {
        let _ = app.emit(STATE_EVENT, json!({ "kind": "started" }));
    }
    result
}

#[tauri::command]
pub fn update_effect(
    node_id: String,
    data: serde_json::Value,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::UpdateEffect {
            node_id,
            data,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn get_device_volume(kind: DeviceKind, name: String) -> Option<f32> {
    #[cfg(target_os = "macos")]
    {
        return crate::audio::macos_hal::device_volume(kind, &name);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (kind, name);
        None
    }
}

#[tauri::command]
pub fn set_device_volume(kind: DeviceKind, name: String, scalar: f32) -> AppResult<()> {
    #[cfg(target_os = "macos")]
    {
        if crate::audio::macos_hal::set_device_volume(kind, &name, scalar) {
            Ok(())
        } else {
            Err(AppError::Device(format!(
                "device {name:?} has no settable {kind:?} volume"
            )))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (kind, name, scalar);
        Err(AppError::Device("device volume control is macOS-only".into()))
    }
}

#[tauri::command]
pub fn stop_pipeline(state: State<'_, AppState>, app: AppHandle) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::Stop { reply: reply_tx })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    let result = reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?;
    if result.is_ok() {
        let _ = app.emit(STATE_EVENT, json!({ "kind": "stopped" }));
    }
    result
}
