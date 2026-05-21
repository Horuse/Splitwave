use std::collections::HashSet;

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[cfg(target_os = "macos")]
use crate::audio::macos_hal;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Input,
    Output,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDeviceInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: &'static str,
}

#[cfg(target_os = "macos")]
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

#[cfg(not(target_os = "macos"))]
pub fn device_info(kind: DeviceKind, name: &str) -> AppResult<NativeDeviceInfo> {
    let device = find(kind, name)?;
    let configs: Vec<cpal::SupportedStreamConfigRange> = match kind {
        DeviceKind::Input => device
            .supported_input_configs()
            .map_err(|e| AppError::Device(e.to_string()))?
            .collect(),
        DeviceKind::Output => device
            .supported_output_configs()
            .map_err(|e| AppError::Device(e.to_string()))?
            .collect(),
    };
    let best = configs
        .into_iter()
        .max_by_key(|c| c.max_sample_rate().0)
        .ok_or_else(|| AppError::Device("device exposes no configs".into()))?
        .with_max_sample_rate();
    Ok(NativeDeviceInfo {
        sample_rate: best.sample_rate().0,
        channels: best.channels(),
        sample_format: "f32",
    })
}

pub fn list_inputs() -> AppResult<Vec<DeviceInfo>> {
    #[cfg(target_os = "macos")]
    {
        return Ok(unique_named(
            macos_hal::list_input_devices().into_iter().map(|d| d.name).collect(),
            DeviceKind::Input,
        ));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let host = cpal::default_host();
        let devices = host
            .input_devices()
            .map_err(|e| AppError::Host(e.to_string()))?;
        Ok(collect_cpal(devices, DeviceKind::Input))
    }
}

pub fn list_outputs() -> AppResult<Vec<DeviceInfo>> {
    #[cfg(target_os = "macos")]
    {
        return Ok(unique_named(
            macos_hal::list_output_devices().into_iter().map(|d| d.name).collect(),
            DeviceKind::Output,
        ));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let host = cpal::default_host();
        let devices = host
            .output_devices()
            .map_err(|e| AppError::Host(e.to_string()))?;
        Ok(collect_cpal(devices, DeviceKind::Output))
    }
}

#[cfg(not(target_os = "macos"))]
fn collect_cpal<I: Iterator<Item = cpal::Device>>(devices: I, kind: DeviceKind) -> Vec<DeviceInfo> {
    devices
        .filter_map(|d| {
            let name = d.name().ok()?;
            Some(DeviceInfo {
                id: name.clone(),
                name,
                kind,
            })
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_inputs_without_panicking() {
        let inputs = list_inputs().expect("inputs");
        println!("found {} input device(s):", inputs.len());
        for d in &inputs {
            println!("  - {}", d.name);
        }
    }

    #[test]
    fn enumerates_outputs_without_panicking() {
        let outputs = list_outputs().expect("outputs");
        println!("found {} output device(s):", outputs.len());
        for d in &outputs {
            println!("  - {}", d.name);
        }
    }
}
