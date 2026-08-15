use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
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

// wpctl reports only a cubic-scale factor; the pulse compatibility layer is the
// one interface that states the device's attenuation in decibels.
fn volume_db(kind: DeviceKind, name: &str) -> Option<f32> {
    let subcommand = match kind {
        DeviceKind::Input => "get-source-volume",
        DeviceKind::Output => "get-sink-volume",
    };
    // pactl accepts a pulse index or a @DEFAULT_*@ alias; the resolved node id
    // doubles as the pulse index, so named devices query by id, not by name.
    let arg = match name {
        "default" | "pipewire" | "sysdefault" => match kind {
            DeviceKind::Input => "@DEFAULT_SOURCE@",
            DeviceKind::Output => "@DEFAULT_SINK@",
        }
        .to_string(),
        _ => resolve_id(kind, name)?.to_string(),
    };
    let out = Command::new("pactl")
        .args([subcommand, &arg])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_db(&String::from_utf8_lossy(&out.stdout))
}

// `pactl subscribe` reports which object changed but not what changed in it,
// and one process serves every watcher, so a change to any sink wakes all
// output watchers. The dispatcher re-reads and drops unchanged values.
struct Subscriber {
    child: Option<Child>,
    poll: Option<Arc<AtomicBool>>,
    next_id: u64,
    watchers: HashMap<u64, (DeviceKind, Notify)>,
}

static SUBSCRIBER: OnceLock<Mutex<Subscriber>> = OnceLock::new();

fn subscriber() -> &'static Mutex<Subscriber> {
    SUBSCRIBER.get_or_init(|| {
        Mutex::new(Subscriber {
            child: None,
            poll: None,
            next_id: 0,
            watchers: HashMap::new(),
        })
    })
}

pub struct Watch {
    id: u64,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let mut sub = subscriber().lock().expect("pactl subscriber poisoned");
        sub.watchers.remove(&self.id);
        if sub.watchers.is_empty() {
            if let Some(mut child) = sub.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            if let Some(stop) = sub.poll.take() {
                stop.store(true, Ordering::Relaxed);
            }
        }
    }
}

pub fn watch_device(kind: DeviceKind, name: &str, notify: Notify) -> Option<Watch> {
    target(kind, name)?;
    let mut sub = subscriber().lock().expect("pactl subscriber poisoned");
    if sub.child.is_none() && sub.poll.is_none() {
        match spawn_subscriber() {
            Some(child) => sub.child = Some(child),
            // No pulse compatibility layer (pipewire without pipewire-pulse):
            // fall back to polling so external volume changes still reach the UI.
            None => sub.poll = Some(spawn_poller()?),
        }
    }
    let id = sub.next_id;
    sub.next_id += 1;
    sub.watchers.insert(id, (kind, notify));
    Some(Watch { id })
}

fn spawn_subscriber() -> Option<Child> {
    let mut child = Command::new("pactl")
        .arg("subscribe")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    thread::Builder::new()
        .name("pactl-subscribe".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Some(kind) = event_kind(&line) else {
                    continue;
                };
                let sub = subscriber().lock().expect("pactl subscriber poisoned");
                for (watched, notify) in sub.watchers.values() {
                    if *watched == kind {
                        notify();
                    }
                }
            }
        })
        .ok()?;
    Some(child)
}

// No pulse module present — poll each watcher so external changes still show.
// The shared dispatcher re-reads and drops unchanged values, so waking on every
// tick is harmless.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

fn spawn_poller() -> Option<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    thread::Builder::new()
        .name("pactl-poll".into())
        .spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let watchers: Vec<Notify> = {
                    let sub = subscriber().lock().expect("pactl subscriber poisoned");
                    sub.watchers
                        .values()
                        .map(|(_, notify)| notify.clone())
                        .collect()
                };
                for notify in watchers {
                    notify();
                }
                thread::sleep(POLL_INTERVAL);
            }
        })
        .ok()?;
    Some(stop)
}

// "Event 'change' on sink #45" — sink-input and source-output are streams.
fn event_kind(line: &str) -> Option<DeviceKind> {
    let object = line.split(" on ").nth(1)?.split_whitespace().next()?;
    match object {
        "sink" => Some(DeviceKind::Output),
        "source" => Some(DeviceKind::Input),
        _ => None,
    }
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
    fn classifies_subscribe_events() {
        use super::event_kind;
        use crate::audio::device::DeviceKind;
        assert_eq!(
            event_kind("Event 'change' on sink #45"),
            Some(DeviceKind::Output)
        );
        assert_eq!(
            event_kind("Event 'change' on source #33"),
            Some(DeviceKind::Input)
        );
        assert_eq!(event_kind("Event 'change' on sink-input #7"), None);
        assert_eq!(event_kind("Event 'new' on client #12"), None);
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
