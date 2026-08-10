use serde::Serialize;

/// `db` is the device's own attenuation, needed to show output meters at the
/// level actually leaving the device. `None` when the backend cannot report it.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceVolume {
    pub scalar: f32,
    pub db: Option<f32>,
}

/// Muted devices report this instead of -inf, which serde emits as `null`.
pub const MUTED_DB: f32 = -120.0;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{device_volume, set_device_volume, watch_device};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{device_volume, set_device_volume, watch_device};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{device_volume, set_device_volume, watch_device};

mod watch;
pub use watch::{unwatch_device_volume, watch_device_volume, Notify};
