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

/// Runs `f` on the Tauri main thread and blocks for its result. Callers must
/// not be the main thread themselves, or this deadlocks.
pub fn run<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Result<R, String> {
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
