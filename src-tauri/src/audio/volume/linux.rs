use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use super::{DeviceVolume, Notify, MUTED_DB};
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

// pipewire-pulse mirrors a sink/source's node id as its pulse index, so a
// resolved id addresses both `wpctl` and `pactl`. Cache it: every volume change
// re-resolves, and a registry snapshot is a full pipewire round-trip.
const ID_TTL: Duration = Duration::from_secs(2);

fn resolve_id(kind: DeviceKind, name: &str) -> Option<u32> {
    let mut cache = id_cache().lock().expect("volume id cache poisoned");
    let key = (kind, name.to_string());
    if let Some(&(id, at)) = cache.get(&key) {
        if at.elapsed() < ID_TTL {
            return Some(id);
        }
    }
    let class = match kind {
        DeviceKind::Input => "Audio/Source",
        DeviceKind::Output => "Audio/Sink",
    };
    let nodes = pw_enum::nodes_by_class(class).ok()?;
    let id = nodes.into_iter().find(|n| n.name == name)?.id;
    cache.insert(key, (id, Instant::now()));
    Some(id)
}

fn id_cache() -> &'static Mutex<HashMap<(DeviceKind, String), (u32, Instant)>> {
    static CACHE: OnceLock<Mutex<HashMap<(DeviceKind, String), (u32, Instant)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
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

// `pactl get-sink-volume` accepts a pulse index or a sink name; pipewire-pulse
// mirrors the node id as the pulse index, but some setups only match by name.
// Try the resolved id first, then fall back to the node name.
fn volume_db(kind: DeviceKind, name: &str) -> Option<f32> {
    let subcommand = match kind {
        DeviceKind::Input => "get-source-volume",
        DeviceKind::Output => "get-sink-volume",
    };
    let candidates: Vec<String> = match name {
        "default" | "pipewire" | "sysdefault" => vec![match kind {
            DeviceKind::Input => "@DEFAULT_SOURCE@",
            DeviceKind::Output => "@DEFAULT_SINK@",
        }
        .to_string()],
        _ => {
            let mut v = Vec::new();
            if let Some(id) = resolve_id(kind, name) {
                v.push(id.to_string());
            }
            v.push(name.to_string());
            v
        }
    };
    for arg in candidates {
        let out = Command::new("pactl")
            .args([subcommand, &arg])
            .output()
            .ok()?;
        if !out.status.success() {
            continue;
        }
        if let Some(db) = parse_db(&String::from_utf8_lossy(&out.stdout)) {
            return Some(db);
        }
    }
    None
}

// A single poller serves every watcher; it wakes the shared dispatcher, which
// re-reads the device and drops unchanged values, so waking on every tick is
// harmless. Polling (rather than `pactl subscribe`) keeps the watch working on
// any PipeWire install — pulse compatibility is optional, and a subscribe that
// spawns but never emits would otherwise leave the slider stale.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

struct Watchers {
    next_id: u64,
    list: HashMap<u64, (DeviceKind, Notify)>,
    stop: Option<Arc<AtomicBool>>,
}

static WATCHERS: OnceLock<Mutex<Watchers>> = OnceLock::new();

fn watchers() -> &'static Mutex<Watchers> {
    WATCHERS.get_or_init(|| {
        Mutex::new(Watchers {
            next_id: 0,
            list: HashMap::new(),
            stop: None,
        })
    })
}

pub struct Watch {
    id: u64,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let mut w = watchers().lock().expect("volume watch registry poisoned");
        w.list.remove(&self.id);
        if w.list.is_empty() {
            if let Some(stop) = w.stop.take() {
                stop.store(true, Ordering::Relaxed);
            }
        }
    }
}

pub fn watch_device(kind: DeviceKind, name: &str, notify: Notify) -> Option<Watch> {
    target(kind, name)?;
    let mut w = watchers().lock().expect("volume watch registry poisoned");
    if w.stop.is_none() {
        w.stop = Some(spawn_poller()?);
    }
    let id = w.next_id;
    w.next_id += 1;
    w.list.insert(id, (kind, notify));
    Some(Watch { id })
}

fn spawn_poller() -> Option<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    thread::Builder::new()
        .name("volume-watch-poll".into())
        .spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let notifies: Vec<Notify> = {
                    let w = watchers().lock().expect("volume watch registry poisoned");
                    w.list.values().map(|(_, notify)| notify.clone()).collect()
                };
                for notify in notifies {
                    notify();
                }
                thread::sleep(POLL_INTERVAL);
            }
        })
        .ok()?;
    Some(stop)
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
