use std::process::Command;

use super::{DeviceVolume, MUTED_DB};
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

pub fn device_volume(kind: DeviceKind, name: &str) -> Option<DeviceVolume> {
    let id = target(kind, name)?;
    let out = Command::new("wpctl")
        .args(["get-volume", &id])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    if s.contains("[MUTED]") {
        return Some(DeviceVolume {
            scalar: 0.0,
            db: Some(MUTED_DB),
        });
    }
    let v: f32 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some(DeviceVolume {
        scalar: v.clamp(0.0, 1.0),
        db: volume_db(kind, name),
    })
}

// wpctl reports only a cubic-scale factor; the pulse compatibility layer is the
// one interface that states the device's attenuation in decibels.
fn volume_db(kind: DeviceKind, name: &str) -> Option<f32> {
    let (subcommand, default) = match kind {
        DeviceKind::Input => ("get-source-volume", "@DEFAULT_SOURCE@"),
        DeviceKind::Output => ("get-sink-volume", "@DEFAULT_SINK@"),
    };
    let target = match name {
        "default" | "pipewire" | "sysdefault" => default,
        _ => name,
    };
    let out = Command::new("pactl")
        .args([subcommand, target])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_db(&String::from_utf8_lossy(&out.stdout))
}

// "front-left: 32768 /  50% / -18.06 dB, front-right: ..." — first channel wins.
fn parse_db(output: &str) -> Option<f32> {
    let tokens: Vec<&str> = output.split_whitespace().collect();
    let idx = tokens
        .iter()
        .position(|t| t.trim_end_matches(',') == "dB")?;
    tokens.get(idx.checked_sub(1)?)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_db;

    #[test]
    fn reads_the_first_channel_decibels() {
        let out = "Volume: front-left: 32768 /  50% / -18.06 dB,   front-right: 32768 /  50% / -18.06 dB\n        balance 0.00\n";
        assert_eq!(parse_db(out), Some(-18.06));
    }

    #[test]
    fn reads_mono_and_zero_decibels() {
        assert_eq!(
            parse_db("Volume: mono: 65536 / 100% / 0.00 dB\n"),
            Some(0.0)
        );
    }

    #[test]
    fn rejects_output_without_decibels() {
        assert_eq!(parse_db("Volume: mono: 65536 / 100%\n"), None);
    }
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
