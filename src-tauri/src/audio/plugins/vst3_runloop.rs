//! The X11 run loop a VST3 plugin expects its host to own.
//!
//! On Linux a plugin does not get its own event loop: it hands the host file
//! descriptors and timers through `IRunLoop` and waits to be called back on the
//! UI thread. Nearly every Linux editor refuses to attach without it.
//!
//! Everything here runs on the Tauri main thread: registration arrives from the
//! plugin during `attached`, and `tick` is driven by the shared plugin ticker.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use vst3::Steinberg::Linux::{
    FileDescriptor, IEventHandler, IEventHandlerTrait, ITimerHandler, ITimerHandlerTrait,
    TimerInterval,
};
use vst3::Steinberg::{kInvalidArgument, kResultOk, tresult};
use vst3::ComRef;

/// Plugin-owned handlers, addressed by pointer because that is the identity the
/// plugin unregisters them by.
#[derive(Default)]
struct Registered {
    events: Vec<(usize, FileDescriptor)>,
    timers: Vec<Timer>,
}

struct Timer {
    handler: usize,
    interval: Duration,
    due: Instant,
}

thread_local! {
    static REGISTERED: RefCell<Registered> = RefCell::new(Registered::default());
}

pub fn register_event_handler(handler: *mut IEventHandler, fd: FileDescriptor) -> tresult {
    if handler.is_null() || fd < 0 {
        return kInvalidArgument;
    }
    REGISTERED.with(|r| r.borrow_mut().events.push((handler as usize, fd)));
    kResultOk
}

pub fn unregister_event_handler(handler: *mut IEventHandler) -> tresult {
    REGISTERED.with(|r| r.borrow_mut().events.retain(|(h, _)| *h != handler as usize));
    kResultOk
}

pub fn register_timer(handler: *mut ITimerHandler, milliseconds: TimerInterval) -> tresult {
    if handler.is_null() || milliseconds == 0 {
        return kInvalidArgument;
    }
    let interval = Duration::from_millis(milliseconds);
    REGISTERED.with(|r| {
        r.borrow_mut().timers.push(Timer {
            handler: handler as usize,
            interval,
            due: Instant::now() + interval,
        })
    });
    kResultOk
}

pub fn unregister_timer(handler: *mut ITimerHandler) -> tresult {
    REGISTERED.with(|r| r.borrow_mut().timers.retain(|t| t.handler != handler as usize));
    kResultOk
}

/// Services every registered descriptor and timer once. Main thread only.
///
/// Handlers are collected before they are called: a plugin is free to register
/// or unregister from inside its own callback, which would otherwise run while
/// the registry is borrowed.
pub fn tick() {
    let ready = ready_descriptors();
    for (handler, fd) in ready {
        // SAFETY: the plugin owns the handler and keeps it alive until it
        // unregisters, which is the only way it leaves the registry.
        unsafe {
            if let Some(handler) = ComRef::from_raw(handler as *mut IEventHandler) {
                handler.onFDIsSet(fd);
            }
        }
    }

    let now = Instant::now();
    let due: Vec<usize> = REGISTERED.with(|r| {
        r.borrow_mut()
            .timers
            .iter_mut()
            .filter(|t| t.due <= now)
            .map(|t| {
                t.due = now + t.interval;
                t.handler
            })
            .collect()
    });
    for handler in due {
        // SAFETY: as above.
        unsafe {
            if let Some(handler) = ComRef::from_raw(handler as *mut ITimerHandler) {
                handler.onTimer();
            }
        }
    }
}

/// Descriptors with input pending, polled without blocking: this runs on the UI
/// thread, so waiting here would freeze the app between plugin events.
fn ready_descriptors() -> Vec<(usize, FileDescriptor)> {
    let registered: Vec<(usize, FileDescriptor)> =
        REGISTERED.with(|r| r.borrow().events.clone());
    if registered.is_empty() {
        return Vec::new();
    }
    let mut fds: Vec<libc::pollfd> = registered
        .iter()
        .map(|(_, fd)| libc::pollfd {
            fd: *fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    // SAFETY: `fds` is a live array of the length passed.
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 0) };
    if ready <= 0 {
        return Vec::new();
    }
    registered
        .into_iter()
        .zip(fds)
        .filter(|(_, poll)| poll.revents != 0)
        .map(|(entry, _)| entry)
        .collect()
}
