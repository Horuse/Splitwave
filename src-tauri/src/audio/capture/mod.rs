#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_backend;
#[cfg(target_os = "macos")]
pub(crate) mod macos_tap;
#[cfg(target_os = "macos")]
pub use macos_backend::{capture_rate, uses_taps, Capture};

#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(target_os = "linux")]
pub use linux::Capture;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{loopback_mix_rate, Capture};

use crate::audio::device::NativeDeviceInfo;
use crate::error::AppResult;

pub fn capture_device_info(
    kind: &str,
    pipeline_sample_rate: Option<u32>,
) -> AppResult<NativeDeviceInfo> {
    #[cfg(target_os = "macos")]
    {
        let _ = (kind, pipeline_sample_rate);
        Ok(NativeDeviceInfo {
            sample_rate: capture_rate(),
            channels: 2,
            sample_format: "f32",
        })
    }
    #[cfg(target_os = "windows")]
    {
        let sample_rate = match kind {
            "system" => loopback_mix_rate().unwrap_or_else(|_| pipeline_sample_rate.unwrap_or(48_000)),
            _ => pipeline_sample_rate.unwrap_or(48_000),
        };
        Ok(NativeDeviceInfo {
            sample_rate,
            channels: 2,
            sample_format: "f32",
        })
    }
    #[cfg(target_os = "linux")]
    {
        let _ = kind;
        Ok(NativeDeviceInfo {
            sample_rate: pipeline_sample_rate.unwrap_or(48_000),
            channels: 2,
            sample_format: "f32",
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (kind, pipeline_sample_rate);
        Ok(NativeDeviceInfo {
            sample_rate: 48_000,
            channels: 2,
            sample_format: "f32",
        })
    }
}
