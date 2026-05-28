use std::process::Command;

use crate::audio::device::DeviceKind;

// wpctl only understands @DEFAULT_*@ or numeric node IDs, so we can only touch
// the default route. Named devices have no handle here, so we skip them.
fn target(kind: DeviceKind, name: &str) -> Option<&'static str> {
    match name {
        "default" | "pipewire" | "sysdefault" => Some(match kind {
            DeviceKind::Input => "@DEFAULT_AUDIO_SOURCE@",
            DeviceKind::Output => "@DEFAULT_AUDIO_SINK@",
        }),
        _ => None,
    }
}

pub fn device_volume(kind: DeviceKind, name: &str) -> Option<f32> {
    let id = target(kind, name)?;
    let out = Command::new("wpctl").args(["get-volume", id]).output().ok()?;
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
        return run(&["set-mute", id, "1"]);
    }
    if !run(&["set-mute", id, "0"]) {
        return false;
    }
    run(&["set-volume", id, &format!("{scalar:.4}")])
}

fn run(args: &[&str]) -> bool {
    Command::new("wpctl")
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
