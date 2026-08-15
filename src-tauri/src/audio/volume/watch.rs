//! Device volume change notifications, shared across the three backends.
//!
//! A backend registers a native listener and calls `Notify` from whatever
//! thread the OS uses. Re-reading the device there would run our property
//! queries on a system callback thread, so the notification only wakes the
//! dispatcher, which reads the volume and emits the Tauri event.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::DeviceVolume;
use crate::audio::device::DeviceKind;
use crate::error::{AppError, AppResult};

pub const VOLUME_EVENT: &str = "audio://device_volume";

/// Called by the backend's native listener; must not block.
pub type Notify = Arc<dyn Fn() + Send + Sync + 'static>;

#[cfg(target_os = "linux")]
use super::linux::Watch;
#[cfg(target_os = "macos")]
use super::macos::Watch;
#[cfg(target_os = "windows")]
use super::windows::Watch;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeChange {
    pub kind: DeviceKind,
    pub name: String,
    pub scalar: f32,
    pub db: Option<f32>,
}

type Key = (DeviceKind, String);

struct Entry {
    /// One device can back several nodes; the listener outlives all of them.
    refs: usize,
    _watch: Watch,
}

static WATCHES: OnceLock<Mutex<HashMap<Key, Entry>>> = OnceLock::new();

fn watches() -> &'static Mutex<HashMap<Key, Entry>> {
    WATCHES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn dispatcher(app: &AppHandle) -> &'static Sender<Key> {
    static TX: OnceLock<Sender<Key>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = channel::<Key>();
        let app = app.clone();
        thread::Builder::new()
            .name("volume-watch".into())
            .spawn(move || {
                // CoreAudio fires once per element and selector, so the same
                // change arrives several times; only forward real movement.
                let mut last: HashMap<Key, (f32, Option<f32>)> = HashMap::new();
                for key in rx {
                    let Some(v) = super::device_volume(key.0, &key.1) else {
                        continue;
                    };
                    if last.insert(key.clone(), (v.scalar, v.db)) == Some((v.scalar, v.db)) {
                        continue;
                    }
                    emit(&app, key, v);
                }
            })
            .expect("spawn volume-watch thread");
        tx
    })
}

fn emit(app: &AppHandle, key: Key, volume: DeviceVolume) {
    let _ = app.emit(
        VOLUME_EVENT,
        VolumeChange {
            kind: key.0,
            name: key.1,
            scalar: volume.scalar,
            db: volume.db,
        },
    );
}

pub fn watch_device_volume(app: &AppHandle, kind: DeviceKind, name: String) -> AppResult<()> {
    let key = (kind, name);
    let mut map = watches().lock().expect("volume watch registry poisoned");
    if let Some(entry) = map.get_mut(&key) {
        entry.refs += 1;
        return Ok(());
    }
    let tx = dispatcher(app).clone();
    let notify_key = key.clone();
    let notify: Notify = Arc::new(move || {
        let _ = tx.send(notify_key.clone());
    });
    let watch = super::watch_device(kind, &key.1, notify).ok_or_else(|| {
        AppError::Device(format!(
            "device {:?} reports no {kind:?} volume changes",
            key.1
        ))
    })?;
    map.insert(
        key,
        Entry {
            refs: 1,
            _watch: watch,
        },
    );
    Ok(())
}

pub fn unwatch_device_volume(kind: DeviceKind, name: String) {
    let key = (kind, name);
    let mut map = watches().lock().expect("volume watch registry poisoned");
    let Some(entry) = map.get_mut(&key) else {
        return;
    };
    entry.refs -= 1;
    if entry.refs == 0 {
        map.remove(&key);
    }
}
