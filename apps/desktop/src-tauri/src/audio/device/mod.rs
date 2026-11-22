use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{device_info, find, list_inputs, list_outputs};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{device_info, list_inputs, list_outputs};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{device_info, find, list_inputs, list_outputs};

/// The name a device goes by, which is also its id in a saved graph.
/// `description().name()` reads the property cpal 0.15's `Device::name` did
/// (CoreAudio `kAudioDevicePropertyDeviceNameCFString`, WASAPI
/// `FriendlyName`), so saved ids keep resolving.
#[cfg(not(target_os = "linux"))]
pub fn cpal_name(device: &cpal::Device) -> Option<String> {
    use cpal::traits::DeviceTrait;
    device.description().ok().map(|d| d.name().to_string())
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
