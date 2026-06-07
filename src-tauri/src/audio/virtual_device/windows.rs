use tauri::AppHandle;

use super::{VirtualDeviceConfig, VirtualDriverStatus};

// installed:false hides the virtual-device UI on the frontend.
pub fn status() -> VirtualDriverStatus {
    VirtualDriverStatus {
        installed: false,
        installed_version: None,
        current_version: super::DRIVER_VERSION,
        needs_update: false,
    }
}

pub fn install(_app: &AppHandle) -> Result<(), String> {
    Err("virtual audio devices are not supported on Windows yet".into())
}

pub fn uninstall() -> Result<(), String> {
    Ok(())
}

pub fn apply_virtual_devices(_devices: Vec<VirtualDeviceConfig>) -> Result<(), String> {
    Err("virtual audio devices are not supported on Windows yet".into())
}
