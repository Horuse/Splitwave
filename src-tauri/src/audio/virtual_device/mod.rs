#[derive(Debug, Clone, serde::Deserialize)]
pub struct VirtualDeviceConfig {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDriverStatus {
    pub installed: bool,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{apply_virtual_devices, install, status, uninstall};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{apply_virtual_devices, install, status, uninstall};
