use serde::{Deserialize, Serialize};

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
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{device_info, find, list_inputs, list_outputs};

#[cfg(not(target_os = "macos"))]
mod linux;
#[cfg(not(target_os = "macos"))]
pub use linux::{device_info, list_inputs, list_outputs};

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
