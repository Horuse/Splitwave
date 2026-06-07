#[derive(Debug, Clone, serde::Deserialize)]
pub struct VirtualDeviceConfig {
    pub id: String,
    pub name: String,
}

// Bump with any driver bundle change; keep in sync with Info.plist CFBundleVersion.
pub const DRIVER_VERSION: u32 = 2;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDriverStatus {
    pub installed: bool,
    pub installed_version: Option<u32>,
    pub current_version: u32,
    pub needs_update: bool,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{apply_virtual_devices, install, status, uninstall};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{apply_virtual_devices, install, status, uninstall};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{apply_virtual_devices, install, status, uninstall};
