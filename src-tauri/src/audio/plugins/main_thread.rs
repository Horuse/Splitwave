//! Getting onto the Tauri main thread, and the tick that drives every host.
//!
//! Formats disagree on how much of their API is main-thread bound -- CLAP and
//! VST3 put nearly everything there, AU only its AppKit half -- but they agree
//! on the mechanism, so it lives here rather than three times over.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

/// How long a main-thread call may wait to start. The UI thread busy this
/// long with something else gets the call withdrawn, so it never runs late
/// for a caller that has already given up on it.
const START_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a call that has started may run. A large plugin can take many
/// seconds to load and is waited for; one past this has hung, and the caller
/// stops waiting rather than hang with it.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);

const QUEUED: u8 = 0;
const STARTED: u8 = 1;
const WITHDRAWN: u8 = 2;

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
    run_via(
        |job| app.run_on_main_thread(job).map_err(|e| e.to_string()),
        f,
        START_TIMEOUT,
        RUN_TIMEOUT,
    )
}

/// `run` over any way of reaching the main thread. A call is withdrawn if it
/// has not started within `start_timeout`; once started it is waited for, up
/// to `run_timeout`.
fn run_via<R: Send + 'static>(
    dispatch: impl FnOnce(Box<dyn FnOnce() + Send>) -> Result<(), String>,
    f: impl FnOnce() -> R + Send + 'static,
    start_timeout: Duration,
    run_timeout: Duration,
) -> Result<R, String> {
    let state = Arc::new(AtomicU8::new(QUEUED));
    let job_state = state.clone();
    let (tx, rx) = mpsc::channel();
    let sent = Instant::now();
    dispatch(Box::new(move || {
        if job_state
            .compare_exchange(QUEUED, STARTED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let _ = tx.send(f());
        }
    }))?;
    if let Ok(r) = rx.recv_timeout(start_timeout) {
        return Ok(r);
    }
    if state
        .compare_exchange(QUEUED, WITHDRAWN, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        return Err("main thread busy; plugin call withdrawn before it started".into());
    }
    rx.recv_timeout(run_timeout.saturating_sub(sent.elapsed()))
        .map_err(|_| {
            tracing::warn!(
                after = ?sent.elapsed(),
                "a plugin call on the main thread never finished; it may still hold an instance"
            );
            "main-thread plugin call hung".to_string()
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// A main thread that picks the job up after `delay`.
    fn after(delay: Duration) -> impl FnOnce(Box<dyn FnOnce() + Send>) -> Result<(), String> {
        move |job| {
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                job();
            });
            Ok(())
        }
    }

    #[test]
    fn a_call_that_never_started_is_withdrawn_and_never_runs() {
        let ran = Arc::new(AtomicBool::new(false));
        let flag = ran.clone();
        let r = run_via(
            after(Duration::from_millis(100)),
            move || flag.store(true, Ordering::SeqCst),
            Duration::from_millis(20),
            Duration::from_secs(5),
        );
        assert!(r.is_err());
        std::thread::sleep(Duration::from_millis(200));
        assert!(!ran.load(Ordering::SeqCst), "a withdrawn call ran anyway");
    }

    #[test]
    fn a_slow_call_that_started_is_waited_for() {
        let r = run_via(
            after(Duration::ZERO),
            || {
                std::thread::sleep(Duration::from_millis(100));
                7
            },
            Duration::from_millis(20),
            Duration::from_secs(5),
        );
        assert_eq!(r, Ok(7));
    }
}
