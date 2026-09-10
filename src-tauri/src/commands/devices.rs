use tauri::AppHandle;
use tracing::info;

use crate::audio::device::{self, DeviceInfo, DeviceKind, NativeDeviceInfo};
use crate::audio::permission::{self, CapturePermission};
use crate::error::{AppError, AppResult};

#[tauri::command]
pub async fn play_cue(device_id: String, muted: bool, gain: f32, beep: bool) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::audio::pipeline::play_cue(&device_id, muted, gain, beep)
    })
    .await
    .map_err(|_| AppError::Stream("cue task failed".into()))?
}

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
pub fn device_info(kind: DeviceKind, name: String) -> AppResult<NativeDeviceInfo> {
    device::device_info(kind, &name)
}

#[tauri::command]
pub fn check_capture_permission() -> CapturePermission {
    let state = permission::capture();
    info!(?state, "capture permission checked");
    state
}

#[tauri::command]
pub fn get_device_volume(
    kind: DeviceKind,
    name: String,
) -> Option<crate::audio::volume::DeviceVolume> {
    crate::audio::volume::device_volume(kind, &name)
}

/// Starts emitting `audio://device_volume` for this device until unwatched.
#[tauri::command]
pub fn watch_device_volume(kind: DeviceKind, name: String, app: AppHandle) -> AppResult<()> {
    crate::audio::volume::watch_device_volume(&app, kind, name)
}

#[tauri::command]
pub fn unwatch_device_volume(kind: DeviceKind, name: String) {
    crate::audio::volume::unwatch_device_volume(kind, name);
}

#[tauri::command]
pub fn set_device_volume(kind: DeviceKind, name: String, scalar: f32) -> AppResult<()> {
    if crate::audio::volume::set_device_volume(kind, &name, scalar) {
        Ok(())
    } else {
        Err(AppError::Device(format!(
            "device {name:?} has no settable {kind:?} volume"
        )))
    }
}
