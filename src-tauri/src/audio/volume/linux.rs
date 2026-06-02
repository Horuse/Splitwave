use std::process::Command;

use crate::audio::device::DeviceKind;
use crate::audio::pw_enum;

// wpctl takes @DEFAULT_*@ or a numeric node id. Default routes use the alias;
// named devices resolve through the registry to their current node id.
fn target(kind: DeviceKind, name: &str) -> Option<String> {
    match name {
        "default" | "pipewire" | "sysdefault" => Some(
            match kind {
                DeviceKind::Input => "@DEFAULT_AUDIO_SOURCE@",
                DeviceKind::Output => "@DEFAULT_AUDIO_SINK@",
            }
            .to_string(),
        ),
        _ => resolve_id(kind, name).map(|id| id.to_string()),
    }
}

fn resolve_id(kind: DeviceKind, name: &str) -> Option<u32> {
    let class = match kind {
        DeviceKind::Input => "Audio/Source",
        DeviceKind::Output => "Audio/Sink",
    };
    let nodes = pw_enum::nodes_by_class(class).ok()?;
    nodes.into_iter().find(|n| n.name == name).map(|n| n.id)
}

pub fn device_volume(kind: DeviceKind, name: &str) -> Option<f32> {
    let id = target(kind, name)?;
    let out = Command::new("wpctl").args(["get-volume", &id]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    if s.contains("[MUTED]") {
        return Some(0.0);
    }
    let v: f32 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some(v.clamp(0.0, 1.0))
}

pub fn set_device_volume(kind: DeviceKind, name: &str, scalar: f32) -> bool {
    let Some(id) = target(kind, name) else {
        return false;
    };
    if scalar <= 0.0 {
        return run(&["set-mute", &id, "1"]);
    }
    if !run(&["set-mute", &id, "0"]) {
        return false;
    }
    run(&["set-volume", &id, &format!("{scalar:.4}")])
}

fn run(args: &[&str]) -> bool {
    Command::new("wpctl")
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
