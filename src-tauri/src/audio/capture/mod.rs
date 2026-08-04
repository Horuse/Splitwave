#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod macos_backend;
#[cfg(target_os = "macos")]
mod macos_tap;
#[cfg(target_os = "macos")]
pub use macos_backend::{capture_rate, uses_taps, Capture};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Capture;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{loopback_mix_rate, Capture};
