//! Getting onto the Tauri main thread, and the tick that drives every host.
//!
//! Formats disagree on how much of their API is main-thread bound -- CLAP and
//! VST3 put nearly everything there, AU only its AppKit half -- but they agree
//! on the mechanism, so it lives here rather than three times over.

use std::sync::mpsc;
use std::sync::Once;
use std::time::Duration;

/// How long a main-thread call may take before the caller gives up. A plugin
/// that blocks the UI thread longer than this has already broken the app; the
/// timeout keeps the calling thread from hanging with it.
const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(5);

use std::sync::OnceLock;
use std::thread::ThreadId;

static MAIN_THREAD_ID: OnceLock<ThreadId> = OnceLock::new();

/// Registers the current thread as the main thread.
pub fn register_main_thread() {
    MAIN_THREAD_ID.get_or_init(|| std::thread::current().id());
}

#[cfg(target_os = "macos")]
extern "C" {
    fn pthread_main_np() -> i32;
}

/// Returns whether the caller is currently on the main thread.
pub fn is_main_thread() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        pthread_main_np() != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        MAIN_THREAD_ID
            .get()
            .is_some_and(|&id| id == std::thread::current().id())
    }
}

/// Runs `f` on the Tauri main thread and blocks for its result. If already on
/// the main thread, runs `f` directly to prevent deadlock.
pub fn run<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Result<R, String> {
    if is_main_thread() {
        return Ok(f());
    }
    let app = crate::app_handle().ok_or_else(|| "app handle not ready".to_string())?;
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(MAIN_THREAD_TIMEOUT)
        .map_err(|_| "main-thread plugin op timed out".to_string())
}

/// Starts the shared 16 ms tick, once. Every host's `tick_and_reclaim` runs on
/// it: plugin timers repaint from it, and it is the only place an instance is
/// freed.
pub fn ensure_ticker() {
    static TICKER: Once = Once::new();
    TICKER.call_once(|| {
        std::thread::Builder::new()
            .name("plugin-timer".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_millis(16));
                if let Some(app) = crate::app_handle() {
                    let _ = app.run_on_main_thread(|| {
                        for host in super::registry::hosts() {
                            host.tick_and_reclaim();
                        }
                    });
                }
            })
            .ok();
    });
}
