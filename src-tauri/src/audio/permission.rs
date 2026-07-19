#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(any(target_os = "linux", target_os = "windows"), allow(dead_code))]
pub enum PermissionState {
    Allowed,
    Denied,
    /// No way to tell without prompting, or the host has no such concept.
    Unknown,
}

/// Which capture permission the active backend needs. Core Audio process taps
/// use System Audio Recording; ScreenCaptureKit uses Screen Recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub enum PermissionKind {
    SystemAudio,
    ScreenRecording,
    /// Linux and Windows capture needs no separate grant.
    None,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct CapturePermission {
    pub kind: PermissionKind,
    pub state: PermissionState,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    // Non-prompting; CGRequestScreenCaptureAccess triggers the dialog instead.
    fn CGPreflightScreenCaptureAccess() -> bool;
}

/// Core Audio exposes no public preflight for the System Audio Recording
/// permission — it is requested implicitly when the first tap is created — so
/// the tap backend reports `Unknown` and failures surface as capture errors.
#[cfg(target_os = "macos")]
pub fn capture() -> CapturePermission {
    if crate::audio::capture::uses_taps() {
        return CapturePermission {
            kind: PermissionKind::SystemAudio,
            state: PermissionState::Unknown,
        };
    }
    CapturePermission {
        kind: PermissionKind::ScreenRecording,
        state: if unsafe { CGPreflightScreenCaptureAccess() } {
            PermissionState::Allowed
        } else {
            PermissionState::Denied
        },
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub fn capture() -> CapturePermission {
    CapturePermission {
        kind: PermissionKind::None,
        state: PermissionState::Unknown,
    }
}
