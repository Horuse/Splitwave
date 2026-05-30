//! Read a device's nominal config straight from CoreAudio HAL (macOS).
//!
//! We never ask cpal "what is this device's default/supported config?":
//!   - `default_*_config` reads the *currently active* CoreAudio stream format,
//!     which is absent for non-default routes (built-in speakers while AirPods
//!     are connected) -> "Invalid property value".
//!   - `supported_*_configs` reads `kAudioStreamPropertyAvailableVirtualFormats`,
//!     which is also empty for those same non-default routes.
//!
//! AUHAL (cpal's underlying output unit on macOS) does NOT need to be told the
//! device's "current" format up front -- it accepts whatever StreamConfig we
//! hand it and asks CoreAudio to convert. So we read the device's nominal
//! sample rate and channel count *directly* from CoreAudio HAL (which works
//! regardless of routing state) and feed those into `build_*_stream`.
//!
//! Sample format is always `f32` -- the universal macOS audio type and the
//! internal pipeline format.

use crate::audio::device::DeviceKind;
use crate::error::{AppError, AppResult};

pub(in crate::audio::pipeline) struct NativeConfig {
    pub config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
    pub sample_rate: u32,
    pub channels: u16,
}

pub(in crate::audio::pipeline) fn native_config(
    kind: DeviceKind,
    _device: &cpal::Device,
    name: &str,
) -> AppResult<NativeConfig> {
    use crate::audio::macos_hal;
    let hal = match kind {
        DeviceKind::Input => macos_hal::find_input_device(name),
        DeviceKind::Output => macos_hal::find_output_device(name),
    }
    .ok_or_else(|| {
        AppError::Device(format!(
            "{kind:?} device {name:?} disappeared between enumeration and open"
        ))
    })?;

    let channels: u16 = hal
        .channels
        .try_into()
        .map_err(|_| AppError::Device(format!("device {name:?} has {} channels (too many)", hal.channels)))?;

    Ok(NativeConfig {
        config: cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(hal.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        },
        sample_format: cpal::SampleFormat::F32,
        sample_rate: hal.sample_rate,
        channels,
    })
}
