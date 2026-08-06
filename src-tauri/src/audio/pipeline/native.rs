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

#[cfg(target_os = "macos")]
pub(in crate::audio::pipeline) fn native_config(
    kind: DeviceKind,
    _device: &cpal::Device,
    name: &str,
    requested_sample_rate: Option<u32>,
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

    let channels: u16 = hal.channels.try_into().map_err(|_| {
        AppError::Device(format!(
            "device {name:?} has {} channels (too many)",
            hal.channels
        ))
    })?;
    // CoreAudio's "available nominal sample rates" is the hardware/driver's
    // native clock list, not the complete set of rates an AUHAL stream can open.
    // Many devices, especially Bluetooth/virtual/aggregate routes, report a
    // tiny or stale nominal list even though CoreAudio can sample-rate-convert
    // an app stream. Try the user's requested stream rate and let the stream
    // builder return the real error if this route truly cannot open it.
    let sample_rate = requested_sample_rate.unwrap_or(hal.sample_rate);

    Ok(NativeConfig {
        config: cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        },
        sample_format: cpal::SampleFormat::F32,
        sample_rate,
        channels,
    })
}

// WASAPI exposes a usable default config for every endpoint (unlike CoreAudio's
// routing quirk), so we read it straight from cpal and let the stream builders
// convert the device-native sample format to/from the f32 pipeline.
#[cfg(target_os = "windows")]
pub(in crate::audio::pipeline) fn native_config(
    kind: DeviceKind,
    device: &cpal::Device,
    name: &str,
    requested_sample_rate: Option<u32>,
) -> AppResult<NativeConfig> {
    use cpal::traits::DeviceTrait;
    let supported = if let Some(rate) = requested_sample_rate {
        let mut configs = match kind {
            DeviceKind::Input => device.supported_input_configs(),
            DeviceKind::Output => device.supported_output_configs(),
        }
        .map_err(|e| AppError::Device(format!("supported configs for {name:?}: {e}")))?;
        configs
            .find(|cfg| cfg.min_sample_rate().0 <= rate && rate <= cfg.max_sample_rate().0)
            .map(|cfg| cfg.with_sample_rate(cpal::SampleRate(rate)))
            .ok_or_else(|| {
                AppError::Device(format!(
                    "{kind:?} device {name:?} does not support {rate} Hz"
                ))
            })?
    } else {
        match kind {
            DeviceKind::Input => device.default_input_config(),
            DeviceKind::Output => device.default_output_config(),
        }
        .map_err(|e| AppError::Device(format!("default config for {name:?}: {e}")))?
    };

    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels();
    Ok(NativeConfig {
        config: cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        },
        sample_format: supported.sample_format(),
        sample_rate,
        channels,
    })
}
