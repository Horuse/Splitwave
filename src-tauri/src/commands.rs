use std::sync::mpsc;

use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tracing::info;

use crate::audio::device::{self, DeviceInfo, DeviceKind, NativeDeviceInfo};
use crate::audio::engine::Command;
use crate::audio::graph::GraphSpec;
use crate::audio::permission::{self, PermissionState};
use crate::audio::system_audio::{self, AudioApplication};
use crate::audio::virtual_device::{self, VirtualDeviceConfig, VirtualDriverStatus};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const STATE_EVENT: &str = "audio://state";

#[tauri::command]
pub fn list_input_devices() -> AppResult<Vec<DeviceInfo>> {
    let devices = device::list_inputs()?;
    info!(count = devices.len(), "input devices listed");
    Ok(devices)
}

#[tauri::command]
pub fn list_output_devices() -> AppResult<Vec<DeviceInfo>> {
    let devices = device::list_outputs()?;
    info!(count = devices.len(), "output devices listed");
    Ok(devices)
}

#[tauri::command]
pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    let apps = system_audio::list_audio_applications()?;
    info!(count = apps.len(), "audio applications listed");
    Ok(apps)
}

#[tauri::command]
pub fn get_app_icons(bundle_ids: Vec<String>) -> std::collections::HashMap<String, String> {
    info!(count = bundle_ids.len(), "loading app icons");
    let icons = system_audio::load_app_icons(bundle_ids);
    info!(loaded = icons.len(), "app icons loaded");
    icons
}

#[tauri::command]
pub fn device_info(kind: DeviceKind, name: String) -> AppResult<NativeDeviceInfo> {
    device::device_info(kind, &name)
}

#[tauri::command]
pub fn check_screen_recording_permission() -> PermissionState {
    let state = permission::screen_recording();
    info!(?state, "screen recording permission checked");
    state
}

#[tauri::command]
pub fn start_pipeline(
    graph: GraphSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    info!(nodes = graph.nodes.len(), "starting pipeline");
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
        info!("pipeline started");
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
pub fn reconcile_pipeline(
    graph: GraphSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    info!(nodes = graph.nodes.len(), "reconciling pipeline");
    let valid = graph.validate()?;
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::Reconcile {
            graph: valid,
            app,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn seek_audio_file(
    node_id: String,
    frame: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::SeekAudioFile {
            node_id,
            frame,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn set_audio_file_loop(
    node_id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::SetAudioFileLoop {
            node_id,
            enabled,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn set_input_volume(
    node_id: String,
    scalar: f32,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::SetInputVolume {
            node_id,
            scalar,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn is_pipeline_running(state: State<'_, AppState>) -> AppResult<bool> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::IsRunning { reply: reply_tx })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))
}

#[tauri::command]
pub fn virtual_driver_status() -> VirtualDriverStatus {
    virtual_device::status()
}

#[tauri::command]
pub fn install_virtual_driver(app: AppHandle) -> Result<(), String> {
    virtual_device::install(&app)
}

#[tauri::command]
pub fn uninstall_virtual_driver() -> Result<(), String> {
    virtual_device::uninstall()
}

#[tauri::command]
pub fn apply_virtual_devices(devices: Vec<VirtualDeviceConfig>) -> Result<(), String> {
    info!(count = devices.len(), "applying virtual devices");
    virtual_device::apply_virtual_devices(devices)
}

#[tauri::command]
pub fn stop_pipeline(state: State<'_, AppState>, app: AppHandle) -> AppResult<()> {
    info!("stopping pipeline");
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::Stop { reply: reply_tx })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    let result = reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?;
    if result.is_ok() {
        info!("pipeline stopped");
        let _ = app.emit(STATE_EVENT, json!({ "kind": "stopped" }));
    }
    result
}
