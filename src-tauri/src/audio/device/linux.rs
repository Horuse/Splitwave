use crate::audio::pw_enum::nodes_by_class;
use crate::error::{AppError, AppResult};

use super::{DeviceInfo, DeviceKind, NativeDeviceInfo};

pub fn device_info(kind: DeviceKind, name: &str) -> AppResult<NativeDeviceInfo> {
    let config_name = name.strip_prefix("monitor:").unwrap_or(name);
    if let Some(config) = crate::audio::virtual_device::config_for_node(config_name) {
        return Ok(NativeDeviceInfo {
            sample_rate: config.sample_rate,
            channels: u16::try_from(config.channels)
                .map_err(|_| AppError::Device(format!("invalid channel count for {name}")))?,
            sample_format: "f32",
        });
    }

    let (class, node_name) = match kind {
        DeviceKind::Input => name
            .strip_prefix("monitor:")
            .map_or(("Audio/Source", name), |sink| ("Audio/Sink", sink)),
        DeviceKind::Output => ("Audio/Sink", name),
    };
    let node = nodes_by_class(class)?
        .into_iter()
        .find(|node| node.name == node_name)
        .ok_or_else(|| AppError::Device(format!("PipeWire node {name:?} not found")))?;
    let sample_rate = node.sample_rate.ok_or_else(|| {
        AppError::Device(format!(
            "PipeWire node {name:?} does not report a sample rate"
        ))
    })?;
    let channels = node.channels.ok_or_else(|| {
        AppError::Device(format!(
            "PipeWire node {name:?} does not report its channel count"
        ))
    })?;
    Ok(NativeDeviceInfo {
        sample_rate,
        channels: u16::try_from(channels)
            .map_err(|_| AppError::Device(format!("invalid channel count for {name}")))?,
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
        let owned = crate::audio::virtual_device::config_for_node(&sink.name).is_some();
        out.push(DeviceInfo {
            id: format!("monitor:{}", sink.name),
            name: if owned {
                sink.description
            } else {
                format!("{} (Monitor)", sink.description)
            },
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
