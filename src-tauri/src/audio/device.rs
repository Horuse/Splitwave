use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

use crate::error::{AppError, AppResult};

/// Direction of an audio device endpoint.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Input,
    Output,
}

/// Lightweight DTO sent to the frontend.
///
/// `id` is the device name from cpal — on macOS device names are stable and unique
/// enough for a demo. For production we'd use a stronger identifier per platform.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
}

/// Enumerate input devices on the default host (CoreAudio on macOS).
pub fn list_inputs() -> AppResult<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| AppError::Host(e.to_string()))?;
    Ok(collect(devices, DeviceKind::Input))
}

/// Enumerate output devices on the default host.
pub fn list_outputs() -> AppResult<Vec<DeviceInfo>> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|e| AppError::Host(e.to_string()))?;
    Ok(collect(devices, DeviceKind::Output))
}

fn collect<I: Iterator<Item = cpal::Device>>(devices: I, kind: DeviceKind) -> Vec<DeviceInfo> {
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

/// Find a device by id (name) on the given side.
#[allow(dead_code)] // wired in by the engine in a later step
pub fn find(kind: DeviceKind, id: &str) -> AppResult<cpal::Device> {
    let host = cpal::default_host();
    let mut iter: Box<dyn Iterator<Item = cpal::Device>> = match kind {
        DeviceKind::Input => Box::new(
            host.input_devices()
                .map_err(|e| AppError::Host(e.to_string()))?,
        ),
        DeviceKind::Output => Box::new(
            host.output_devices()
                .map_err(|e| AppError::Host(e.to_string()))?,
        ),
    };
    iter.find(|d| d.name().map(|n| n == id).unwrap_or(false))
        .ok_or_else(|| AppError::Device(format!("device not found: {id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_inputs_without_panicking() {
        let inputs = list_inputs().expect("inputs");
        // CI runners may have zero devices — only assert that the call returns Ok.
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
