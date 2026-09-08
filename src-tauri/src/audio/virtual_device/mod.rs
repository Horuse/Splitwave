#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDeviceConfig {
    pub id: String,
    pub name: String,
    #[serde(default = "default_channels")]
    pub channels: u32,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
}

fn default_channels() -> u32 {
    2
}

fn default_sample_rate() -> u32 {
    48_000
}

// Bump with any driver bundle change; keep in sync with Info.plist CFBundleVersion.
pub const DRIVER_VERSION: u32 = 5;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDriverStatus {
    pub installed: bool,
    pub installed_version: Option<u32>,
    pub current_version: u32,
    pub needs_update: bool,
}

pub mod windows_cable;
pub use windows_cable::{
    install_windows_virtual_cable, windows_virtual_cable_status, WindowsVirtualCableError,
    WindowsVirtualCableStatus,
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{apply_virtual_devices, install, status, uninstall};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{apply_virtual_devices, find_virtual_device, install, status, uninstall};

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub fn find_virtual_device(_id_or_name: &str) -> Option<VirtualDeviceConfig> {
    None
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{apply_virtual_devices, install, status, uninstall};

#[cfg(test)]
mod tests {
    use super::VirtualDeviceConfig;

    #[test]
    fn deserializes_with_default_sample_rate() {
        let json = r#"{"id":"v1","name":"Virtual Mic","channels":2}"#;
        let cfg: VirtualDeviceConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.sample_rate, 48_000);
        assert_eq!(cfg.channels, 2);
    }

    #[test]
    fn deserializes_with_custom_sample_rate() {
        let json = r#"{"id":"v1","name":"Virtual Mic","channels":2,"sampleRate":96000}"#;
        let cfg: VirtualDeviceConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.sample_rate, 96_000);
    }
}
