#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub enum PermissionState {
    Allowed,
    Denied,
    /// Returned on non-macOS hosts (no screen recording concept).
    #[allow(dead_code)]
    Unknown,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    // Non-prompting; CGRequestScreenCaptureAccess triggers the dialog instead.
    fn CGPreflightScreenCaptureAccess() -> bool;
}

#[cfg(target_os = "macos")]
pub fn screen_recording() -> PermissionState {
    if unsafe { CGPreflightScreenCaptureAccess() } {
        PermissionState::Allowed
    } else {
        PermissionState::Denied
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn screen_recording() -> PermissionState {
    PermissionState::Unknown
}
