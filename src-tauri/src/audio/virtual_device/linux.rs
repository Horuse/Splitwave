use tauri::AppHandle;

use super::{VirtualDeviceConfig, VirtualDriverStatus};

// Linux virtual devices via PipeWire config snippets are planned;
// until that lands, report not installed and reject mutating calls.

pub fn status() -> VirtualDriverStatus {
    VirtualDriverStatus { installed: false }
}

pub fn install(_app: &AppHandle) -> Result<(), String> {
    Err("virtual driver install is not supported on this platform yet".into())
}

pub fn uninstall() -> Result<(), String> {
    Ok(())
}

pub fn apply_virtual_devices(_devices: Vec<VirtualDeviceConfig>) -> Result<(), String> {
    Err("virtual devices are not supported on this platform yet".into())
}
