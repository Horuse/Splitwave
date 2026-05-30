use crate::audio::pw_enum::nodes_by_class;
use crate::error::AppResult;

use super::{DeviceInfo, DeviceKind, NativeDeviceInfo};

pub fn device_info(_kind: DeviceKind, _name: &str) -> AppResult<NativeDeviceInfo> {
    // PipeWire converts to whatever we request, so the graph always runs at
    // 48 kHz stereo f32.
    Ok(NativeDeviceInfo {
        sample_rate: 48_000,
        channels: 2,
        sample_format: "f32",
    })
}

pub fn list_inputs() -> AppResult<Vec<DeviceInfo>> {
    let mut out: Vec<DeviceInfo> = nodes_by_class("Audio/Source")?
        .into_iter()
        .map(|n| DeviceInfo { id: n.name, name: n.description, kind: DeviceKind::Input })
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
        .map(|n| DeviceInfo { id: n.name, name: n.description, kind: DeviceKind::Output })
        .collect())
}
