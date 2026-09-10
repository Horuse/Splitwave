use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tracing::info;

use crate::audio::engine::Command;
use crate::audio::virtual_device::{self, VirtualDeviceConfig, VirtualDriverStatus};
use crate::error::AppError;
use crate::state::AppState;

use super::helpers::{audio_request, STATE_EVENT};

#[tauri::command]
pub fn virtual_driver_status() -> VirtualDriverStatus {
    virtual_device::status()
}

#[tauri::command]
pub async fn windows_virtual_cable_status(
) -> Result<virtual_device::WindowsVirtualCableStatus, virtual_device::WindowsVirtualCableError> {
    tauri::async_runtime::spawn_blocking(virtual_device::windows_virtual_cable_status)
        .await
        .map_err(|_| {
            virtual_device::WindowsVirtualCableError::operation_failed(
                "Status query stopped unexpectedly",
            )
        })?
}

#[tauri::command]
pub async fn install_windows_virtual_cable(
) -> Result<virtual_device::WindowsVirtualCableStatus, virtual_device::WindowsVirtualCableError> {
    tauri::async_runtime::spawn_blocking(virtual_device::install_windows_virtual_cable)
        .await
        .map_err(|_| {
            virtual_device::WindowsVirtualCableError::operation_failed(
                "Installation task stopped unexpectedly",
            )
        })?
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
pub async fn apply_virtual_devices(
    devices: Vec<VirtualDeviceConfig>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    info!(count = devices.len(), "applying virtual devices");
    // Reloading the driver yanks its devices; a pipeline holding one wedges mid-call.
    let tx = state.audio_tx.clone();
    let stopped = match audio_request(tx, |reply| Command::Stop { reply })
        .await
        .map_err(|e| e.to_string())?
    {
        Ok(()) => true,
        // An idle engine already satisfies what Stop is here to guarantee.
        Err(AppError::NotRunning) => false,
        Err(e) => return Err(e.to_string()),
    };
    if stopped {
        let _ = app.emit(STATE_EVENT, json!({ "kind": "stopped" }));
    }
    tauri::async_runtime::spawn_blocking(move || virtual_device::apply_virtual_devices(devices))
        .await
        .map_err(|_| "virtual device task failed".to_string())?
}
