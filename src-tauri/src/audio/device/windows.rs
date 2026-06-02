use std::collections::HashSet;

use cpal::traits::{DeviceTrait, HostTrait};

use crate::error::{AppError, AppResult};

use super::{DeviceInfo, DeviceKind, NativeDeviceInfo};

pub fn device_info(kind: DeviceKind, name: &str) -> AppResult<NativeDeviceInfo> {
    let device = find(kind, name)?;
    let config = match kind {
        DeviceKind::Input => device.default_input_config(),
        DeviceKind::Output => device.default_output_config(),
    }
    .map_err(|e| AppError::Device(format!("default config for {name:?}: {e}")))?;
    Ok(NativeDeviceInfo {
        sample_rate: config.sample_rate().0,
        channels: config.channels(),
        sample_format: "f32",
    })
}

pub fn list_inputs() -> AppResult<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|e| AppError::Host(e.to_string()))?;
    Ok(unique_named(devices, DeviceKind::Input))
}

pub fn list_outputs() -> AppResult<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let devices = host.output_devices().map_err(|e| AppError::Host(e.to_string()))?;
    Ok(unique_named(devices, DeviceKind::Output))
}

pub fn find(kind: DeviceKind, id: &str) -> AppResult<cpal::Device> {
    let host = cpal::default_host();
    let mut devices = match kind {
        DeviceKind::Input => host.input_devices(),
        DeviceKind::Output => host.output_devices(),
    }
    .map_err(|e| AppError::Host(e.to_string()))?;
    devices
        .find(|d| d.name().map(|n| n == id).unwrap_or(false))
        .ok_or_else(|| AppError::Device(format!("device not found: {id}")))
}

fn unique_named(devices: impl Iterator<Item = cpal::Device>, kind: DeviceKind) -> Vec<DeviceInfo> {
    let mut seen = HashSet::new();
    devices
        .filter_map(|d| d.name().ok())
        .filter(|n| seen.insert(n.clone()))
        .map(|name| DeviceInfo {
            id: name.clone(),
            name,
            kind,
        })
        .collect()
}
