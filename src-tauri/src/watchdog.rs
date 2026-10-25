//! Detects a blocked main thread. A "Not responding" window that the user or
//! the OS then kills leaves no panic and no signal, only an unclean exit. The
//! marker written here while the thread is stuck lets the next launch report a
//! hang and name the command it was stuck in.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::AppHandle;
use tracing::warn;

const PROBE_EVERY: Duration = Duration::from_secs(1);
const HANG_AFTER: Duration = Duration::from_secs(5);
/// A probe that wakes this late slept through a system suspend; the main
/// thread was not blocked meanwhile.
const SUSPEND_GAP: Duration = Duration::from_secs(3);
const MARKER: &str = "hang.json";

static MARKER_PATH: OnceLock<PathBuf> = OnceLock::new();
static MAIN_COMMAND: Mutex<Option<String>> = Mutex::new(None);

pub struct CommandGuard;

impl Drop for CommandGuard {
    fn drop(&mut self) {
        if let Ok(mut current) = MAIN_COMMAND.lock() {
            *current = None;
        }
    }
}

/// Wraps every IPC dispatch. Sync commands run inline on the main thread, so
/// while one is stuck its name is what this holds.
pub fn enter_command(name: &str) -> CommandGuard {
    if let Ok(mut current) = MAIN_COMMAND.lock() {
        *current = Some(name.to_string());
    }
    CommandGuard
}

/// The hang the previous session was stuck in when it died, if any.
pub fn take_previous_hang(dir: &Path) -> Option<serde_json::Value> {
    let path = dir.join(MARKER);
    let contents = std::fs::read_to_string(&path).ok();
    let _ = std::fs::remove_file(&path);
    serde_json::from_str(&contents?).ok()
}

pub fn clear_marker() {
    if let Some(path) = MARKER_PATH.get() {
        let _ = std::fs::remove_file(path);
    }
}

pub fn start(app: AppHandle, dir: &Path) {
    let marker = dir.join(MARKER);
    let _ = MARKER_PATH.set(marker.clone());
    let beat = Arc::new(Mutex::new(Instant::now()));
    // One probe in flight at a time, so a stuck thread doesn't pile them up.
    let pending = Arc::new(AtomicBool::new(false));
    let _ = thread::Builder::new()
        .name("main-watchdog".into())
        .spawn(move || {
            let mut last_tick = Instant::now();
            let mut worst: Option<Duration> = None;
            loop {
                thread::sleep(PROBE_EVERY);
                let now = Instant::now();
                if now.duration_since(last_tick) > PROBE_EVERY + SUSPEND_GAP {
                    *beat.lock().unwrap() = now;
                }
                last_tick = now;

                if !pending.swap(true, Ordering::AcqRel) {
                    let beat = beat.clone();
                    let done = pending.clone();
                    let posted = app.run_on_main_thread(move || {
                        *beat.lock().unwrap() = Instant::now();
                        done.store(false, Ordering::Release);
                    });
                    if posted.is_err() {
                        pending.store(false, Ordering::Release);
                    }
                }

                let stalled = now.saturating_duration_since(*beat.lock().unwrap());
                if stalled >= HANG_AFTER {
                    let command = MAIN_COMMAND.lock().ok().and_then(|c| c.clone());
                    if worst.is_none() {
                        warn!(?command, "main thread unresponsive");
                    }
                    worst = Some(stalled);
                    write_marker(&marker, stalled, command.as_deref());
                } else if let Some(blocked) = worst.take() {
                    let _ = std::fs::remove_file(&marker);
                    warn!(
                        blocked_ms = blocked.as_millis() as u64,
                        "main thread responsive again"
                    );
                }
            }
        });
}

fn write_marker(path: &Path, stalled: Duration, command: Option<&str>) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let payload = json!({
        "stalledMs": stalled.as_millis() as u64,
        "command": command,
        "ts": ts,
    });
    let _ = std::fs::write(path, payload.to_string());
}
