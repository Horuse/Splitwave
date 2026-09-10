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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowsVirtualCableState {
    NotInstalled,
    InstalledExternal,
    InstalledManaged,
    Partial,
    RebootRequired,
    RemovalPendingReboot,
    UnknownOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowsVirtualCableOwnership {
    External,
    Managed,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsVirtualCableStatus {
    pub state: WindowsVirtualCableState,
    pub usable: bool,
    pub provider: String,
    pub installed_version: Option<String>,
    pub render_endpoint_name: Option<String>,
    pub capture_endpoint_name: Option<String>,
    pub ownership: WindowsVirtualCableOwnership,
    pub managed_by_splitwave: bool,
    pub reboot_required: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsVirtualCableError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installer_exit_code: Option<i32>,
}

impl WindowsVirtualCableError {
    pub fn operation_failed(message: impl Into<String>) -> Self {
        Self::new("operationFailed", message)
    }

    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            installer_exit_code: None,
        }
    }
}

impl std::fmt::Display for WindowsVirtualCableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WindowsVirtualCableError {}

#[cfg(target_os = "windows")]
pub mod windows_cable;
#[cfg(target_os = "windows")]
pub use windows_cable::{install_windows_virtual_cable, windows_virtual_cable_status};

#[cfg(not(target_os = "windows"))]
pub fn windows_virtual_cable_status() -> Result<WindowsVirtualCableStatus, WindowsVirtualCableError>
{
    Err(WindowsVirtualCableError::new(
        "unsupportedPlatform",
        "VB-CABLE integration is available only on Windows",
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn install_windows_virtual_cable() -> Result<WindowsVirtualCableStatus, WindowsVirtualCableError>
{
    windows_virtual_cable_status()
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{apply_virtual_devices, install, status, uninstall};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{apply_virtual_devices, config_for_node, install, restore, status, uninstall};

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
