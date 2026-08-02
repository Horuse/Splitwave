use crate::audio::device::DeviceKind;
use crate::audio::pw_ctl::{self, DefaultRoute};
use crate::audio::pw_enum;

fn route(kind: DeviceKind) -> DefaultRoute {
    match kind {
        DeviceKind::Input => DefaultRoute::Source,
        DeviceKind::Output => DefaultRoute::Sink,
    }
}

fn media_class(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Input => "Audio/Source",
        DeviceKind::Output => "Audio/Sink",
    }
}

// Default routes resolve through the "default" metadata object; named devices
// resolve straight through the registry to their current node id.
fn resolve_id(kind: DeviceKind, name: &str) -> Option<u32> {
    let target = match name {
        "default" | "pipewire" | "sysdefault" => pw_ctl::default_node_name(route(kind)).ok()??,
        _ => name.to_string(),
    };
    let nodes = pw_enum::nodes_by_class(media_class(kind)).ok()?;
    nodes.into_iter().find(|n| n.name == target).map(|n| n.id)
}

pub fn device_volume(kind: DeviceKind, name: &str) -> Option<f32> {
    let id = resolve_id(kind, name)?;
    let volume = pw_ctl::node_volume(id).ok()?;
    if volume.mute {
        return Some(0.0);
    }
    // channelVolumes is per channel; the UI carries a single scalar
    let peak = volume.channel_volumes.into_iter().fold(0.0f32, f32::max);
    Some(peak.clamp(0.0, 1.0))
}

pub fn set_device_volume(kind: DeviceKind, name: &str, scalar: f32) -> bool {
    let Some(id) = resolve_id(kind, name) else {
        return false;
    };
    let Ok(current) = pw_ctl::node_volume(id) else {
        return false;
    };
    let channels = current.channel_volumes.len();
    if scalar <= 0.0 {
        return pw_ctl::set_node_volume(id, channels, 0.0, true).is_ok();
    }
    pw_ctl::set_node_volume(id, channels, scalar, false).is_ok()
}
