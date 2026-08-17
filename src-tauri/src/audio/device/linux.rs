use crate::audio::pw_enum::{nodes_by_class, PwNode};
use crate::error::{AppError, AppResult};

use super::{DeviceInfo, DeviceKind, NativeDeviceInfo};

pub fn device_info(kind: DeviceKind, name: &str) -> AppResult<NativeDeviceInfo> {
    let (class, node_name) = match (kind, name.strip_prefix("monitor:")) {
        (DeviceKind::Input, Some(sink)) => ("Audio/Sink", sink),
        (DeviceKind::Input, None) => ("Audio/Source", name),
        (DeviceKind::Output, _) => ("Audio/Sink", name),
    };
    let node = nodes_by_class(class)?
        .into_iter()
        .find(|node| node.name == node_name)
        .ok_or_else(|| AppError::Device(format!("input/output device not found: {name}")))?;
    Ok(native_info(&node))
}

fn native_info(node: &PwNode) -> NativeDeviceInfo {
    // PipeWire negotiates the requested f32 format while preserving the
    // physical node's clock-domain width exposed by its node properties.
    NativeDeviceInfo {
        sample_rate: node.sample_rate,
        channels: node.channels,
        sample_format: "f32",
    }
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
