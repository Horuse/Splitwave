use std::collections::HashSet;

use cpal::traits::{DeviceTrait, HostTrait};

use crate::audio::macos_hal;
use crate::error::{AppError, AppResult};

use super::{DeviceInfo, DeviceKind, NativeDeviceInfo};

pub fn device_info(kind: DeviceKind, name: &str) -> AppResult<NativeDeviceInfo> {
    let hal = match kind {
        DeviceKind::Input => macos_hal::find_input_device(name),
        DeviceKind::Output => macos_hal::find_output_device(name),
    }
    .ok_or_else(|| AppError::Device(format!("device not found: {name}")))?;
    let channels: u16 = hal
        .channels
        .try_into()
        .map_err(|_| AppError::Device(format!("device {name:?} has {} channels", hal.channels)))?;
    Ok(NativeDeviceInfo {
        sample_rate: hal.sample_rate,
        channels,
        sample_format: "f32",
    })
}

pub fn list_inputs() -> AppResult<Vec<DeviceInfo>> {
    Ok(unique_named(
        macos_hal::list_input_devices().into_iter().map(|d| d.name).collect(),
        DeviceKind::Input,
    ))
}

pub fn list_outputs() -> AppResult<Vec<DeviceInfo>> {
    Ok(unique_named(
        macos_hal::list_output_devices().into_iter().map(|d| d.name).collect(),
        DeviceKind::Output,
    ))
}

fn unique_named(names: Vec<String>, kind: DeviceKind) -> Vec<DeviceInfo> {
    let mut seen = HashSet::new();
    names
        .into_iter()
        .filter(|n| seen.insert(n.clone()))
        .map(|name| DeviceInfo {
            id: name.clone(),
            name,
            kind,
        })
        .collect()
}

// `host.devices()` returns is_default=false -> cpal uses HalOutput bound to a
// specific AudioDeviceID. `default_*_device` returns is_default=true -> cpal
// uses DefaultOutput which silently follows the system default when it changes.
pub fn find(kind: DeviceKind, id: &str) -> AppResult<cpal::Device> {
    let host = cpal::default_host();
    let matches: Vec<cpal::Device> = host
        .devices()
        .map_err(|e| AppError::Host(e.to_string()))?
        .filter(|d| d.name().map(|n| n == id).unwrap_or(false))
        .collect();
    if matches.len() < 2 {
        return matches
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Device(format!("device not found: {id}")));
    }
    // Bluetooth headsets expose same-named input- and output-scope devices; pick the one valid in the requested scope.
    let in_scope = |d: &cpal::Device| {
        match kind {
            DeviceKind::Input => d.supported_input_configs().map(|mut c| c.next().is_some()),
            DeviceKind::Output => d.supported_output_configs().map(|mut c| c.next().is_some()),
        }
        .unwrap_or(false)
    };
    matches
        .iter()
        .find(|d| in_scope(d))
        .or(matches.first())
        .cloned()
        .ok_or_else(|| AppError::Device(format!("device not found: {id}")))
}
