use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use libpulse_binding::callbacks::ListResult;
use libpulse_binding::context::subscribe::{Facility, InterestMaskSet};
use libpulse_binding::context::introspect::Introspector;
use libpulse_binding::context::{Context, FlagSet, State};
use libpulse_binding::mainloop::threaded::Mainloop;
use libpulse_binding::operation;
use libpulse_binding::volume::{ChannelVolumes, Volume, VolumeDB, VolumeLinear};

use super::{DeviceVolume, Notify, MUTED_DB};
use crate::audio::device::DeviceKind;

const APP: &str = "splitwave";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Runs a one-shot PulseAudio operation on its own connection. Each call makes
/// a fresh connection, which is cheap and keeps reads/ writes independent of the
/// long-lived watch connection.
fn with_connection<T>(f: impl FnOnce(&mut Context) -> T) -> Option<T> {
    let mut mainloop = Mainloop::new()?;
    let mut context = Context::new(&mainloop, APP)?;
    context.connect(None, FlagSet::NOFLAGS, None).ok()?;
    mainloop.start().ok()?;
    let ready = wait_ready(&context);
    let out = ready.then(|| f(&mut context));
    context.disconnect();
    mainloop.stop();
    out
}

fn wait_ready(context: &Context) -> bool {
    let start = Instant::now();
    while start.elapsed() < CONNECT_TIMEOUT {
        match context.get_state() {
            State::Ready => return true,
            State::Failed | State::Terminated => return false,
            _ => thread::sleep(Duration::from_millis(5)),
        }
    }
    false
}

fn wait_done<C: ?Sized>(op: &operation::Operation<C>) -> bool {
    let start = Instant::now();
    while start.elapsed() < CONNECT_TIMEOUT {
        if op.get_state() == operation::State::Done {
            return true;
        }
        thread::sleep(Duration::from_millis(2));
    }
    false
}

fn device_volume_from(volume: &ChannelVolumes, mute: bool) -> DeviceVolume {
    let avg: Volume = volume.avg();
    if mute {
        return DeviceVolume {
            scalar: 0.0,
            db: Some(MUTED_DB),
        };
    }
    let lin: VolumeLinear = avg.into();
    let db: VolumeDB = avg.into();
    DeviceVolume {
        // PulseAudio's linear volume is the device volume cubed; undo that so
        // the scalar matches what PipeWire reports (0..1 device volume).
        scalar: lin.0.cbrt().clamp(0.0, 1.0) as f32,
        db: Some(db.0 as f32).filter(|db| db.is_finite()),
    }
}

// "default"/"pipewire"/"sysdefault" are route aliases, not PulseAudio sink
// names; resolve them to the server's actual default sink/source name.
fn resolve_device(intro: &Introspector, kind: DeviceKind, name: &str) -> Option<String> {
    match name {
        "default" | "pipewire" | "sysdefault" => {
            let out = Arc::new(Mutex::new(None));
            let out_cb = out.clone();
            let op = intro.get_server_info(move |info| {
                let n = match kind {
                    DeviceKind::Output => info.default_sink_name.clone(),
                    DeviceKind::Input => info.default_source_name.clone(),
                };
                *out_cb.lock().unwrap() = n.map(|c| c.into_owned());
            });
            let _ = wait_done(&op);
            let result = out.lock().unwrap().take();
            result
        }
        _ => Some(name.to_string()),
    }
}

pub fn device_volume(kind: DeviceKind, name: &str) -> Option<DeviceVolume> {
    let out = Arc::new(Mutex::new(None));
    let out_cb = out.clone();
    with_connection(move |context| {
        let intro = context.introspect();
        let real = resolve_device(&intro, kind, name);
        if let Some(real) = real {
            match kind {
                DeviceKind::Output => {
                    let op = intro.get_sink_info_by_name(&real, move |res| {
                        if let ListResult::Item(info) = res {
                            *out_cb.lock().unwrap() =
                                Some(device_volume_from(&info.volume, info.mute));
                        }
                    });
                    let _ = wait_done(&op);
                }
                DeviceKind::Input => {
                    let op = intro.get_source_info_by_name(&real, move |res| {
                        if let ListResult::Item(info) = res {
                            *out_cb.lock().unwrap() =
                                Some(device_volume_from(&info.volume, info.mute));
                        }
                    });
                    let _ = wait_done(&op);
                }
            }
        }
    })?;
    let result = out.lock().unwrap().take();
    result
}

pub fn set_device_volume(kind: DeviceKind, name: &str, scalar: f32) -> bool {
    let mute = scalar <= 0.0;
    let mut volumes = ChannelVolumes::default();
    if !mute {
        let s = scalar.clamp(0.0, 1.0);
        // Inverse of the read-side cbrt: request the device volume `s`.
        let v: Volume = VolumeLinear((s * s * s) as f64).into();
        volumes.set(ChannelVolumes::CHANNELS_MAX, v);
    }
    with_connection(move |context| {
        let mut intro = context.introspect();
        let Some(real) = resolve_device(&intro, kind, name) else {
            return false;
        };
        let (set_vol, set_mute) = match kind {
            DeviceKind::Output => (
                intro.set_sink_volume_by_name(&real, &volumes, None),
                intro.set_sink_mute_by_name(&real, mute, None),
            ),
            DeviceKind::Input => (
                intro.set_source_volume_by_name(&real, &volumes, None),
                intro.set_source_mute_by_name(&real, mute, None),
            ),
        };
        let vol_ok = wait_done(&set_vol);
        let mute_ok = wait_done(&set_mute);
        vol_ok && mute_ok
    })
    .unwrap_or(false)
}

// A single PulseAudio connection serves every watcher. `subscribe` reports which
// facility changed, so any sink change wakes all output watchers; the shared
// dispatcher re-reads and drops unchanged values.
struct Watchers {
    next_id: u64,
    list: HashMap<u64, (DeviceKind, Notify)>,
    stop: Option<Arc<AtomicBool>>,
}

static WATCHERS: OnceLock<Mutex<Watchers>> = OnceLock::new();

fn watchers() -> &'static Mutex<Watchers> {
    WATCHERS.get_or_init(|| Mutex::new(Watchers {
        next_id: 0,
        list: HashMap::new(),
        stop: None,
    }))
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
    // Validate the device resolves through PulseAudio before watching it.
    device_volume(kind, name)?;
    let mut w = watchers().lock().expect("volume watch registry poisoned");
    if w.stop.is_none() {
        w.stop = Some(spawn_watcher()?);
    }
    let id = w.next_id;
    w.next_id += 1;
    w.list.insert(id, (kind, notify));
    Some(Watch { id })
}

fn spawn_watcher() -> Option<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    thread::Builder::new()
        .name("pulse-subscribe".into())
        .spawn(move || {
            let Some(mut mainloop) = Mainloop::new() else {
                return;
            };
            let mut context = match Context::new(&mainloop, APP) {                Some(c) => c,
                None => return,
            };
            if context.connect(None, FlagSet::NOFLAGS, None).is_err() {
                return;
            }
            if mainloop.start().is_err() {
                return;
            }
            if !wait_ready(&context) {
                context.disconnect();
                mainloop.stop();
                return;
            }
            context.set_subscribe_callback(Some(Box::new(|facility, _op, _index| {
                let kind = match facility {
                    Some(Facility::Sink) => Some(DeviceKind::Output),
                    Some(Facility::Source) => Some(DeviceKind::Input),
                    _ => None,
                };
                if let Some(kind) = kind {
                    let w = watchers().lock().expect("volume watch registry poisoned");
                    for (_, (watched, notify)) in w.list.iter() {
                        if *watched == kind {
                            notify();
                        }
                    }
                }
            })));
            let _ = context.subscribe(
                InterestMaskSet::SINK | InterestMaskSet::SOURCE,
                |_ok: bool| {},
            );
            while !flag.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(50));
            }
            context.disconnect();
            mainloop.stop();
        })
        .ok()?;
    Some(stop)
}
