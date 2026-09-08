use crate::audio::pw_enum::nodes_by_class;
use crate::error::AppResult;

use super::{DeviceInfo, DeviceKind, NativeDeviceInfo};

pub fn device_info(kind: DeviceKind, name: &str) -> AppResult<NativeDeviceInfo> {
    let clean_name = name.strip_prefix("monitor:").unwrap_or(name);

    // Check if it's one of Splitwave's virtual devices first
    if let Some(vd) = crate::audio::virtual_device::find_virtual_device(clean_name) {
        return Ok(NativeDeviceInfo {
            sample_rate: vd.sample_rate,
            channels: vd.channels as u16,
            sample_format: "f32",
        });
    }

    // Otherwise check PipeWire nodes
    let class = match kind {
        DeviceKind::Input if name.starts_with("monitor:") => "Audio/Sink",
        DeviceKind::Input => "Audio/Source",
        DeviceKind::Output => "Audio/Sink",
    };
    if let Ok(nodes) = nodes_by_class(class) {
        if let Some(node) = nodes.into_iter().find(|n| n.name == clean_name) {
            return Ok(NativeDeviceInfo {
                sample_rate: node.sample_rate.unwrap_or(48_000),
                channels: node.channels.unwrap_or(2) as u16,
                sample_format: "f32",
            });
        }
    }

    Ok(NativeDeviceInfo {
        sample_rate: 48_000,
        channels: 2,
        sample_format: "f32",
    })
}

pub fn list_inputs() -> AppResult<Vec<DeviceInfo>> {
    let mut out: Vec<DeviceInfo> = nodes_by_class("Audio/Source")?
        .into_iter()
        .map(|n| DeviceInfo {
            id: n.name,
            name: n.description,
            kind: DeviceKind::Input,
        })
        .collect();
    // Every sink exposes a monitor we can record; offer them as inputs too.
    for sink in nodes_by_class("Audio/Sink")? {
        out.push(DeviceInfo {
            id: format!("monitor:{}", sink.name),
            name: format!("{} (Monitor)", sink.description),
            kind: DeviceKind::Input,
        });
    }
    Ok(out)
}

pub fn list_outputs() -> AppResult<Vec<DeviceInfo>> {
    Ok(nodes_by_class("Audio/Sink")?
        .into_iter()
        .map(|n| DeviceInfo {
            id: n.name,
            name: n.description,
            kind: DeviceKind::Output,
        })
        .collect())
}
