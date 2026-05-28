//! Rust binding for the Swift `SCKAudioCapture` static library (native/).
//!
//! Architecture: the Swift side owns the `SCStream` and its delegate; on each
//! audio `CMSampleBuffer` it interleaves f32 samples and invokes our C
//! callback, which pushes them into a `rtrb::Producer<f32>` shared with the
//! pipeline engine.
//!
//! All public functions block on Swift's async start completion via a
//! `DispatchSemaphore` on the Swift side; the wait is bounded (10 s).

use std::cell::UnsafeCell;
use std::ffi::{c_void, CString};
use std::mem::ManuallyDrop;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tracing::{info, warn};

use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultCode {
    Ok = 0,
    OsVersion = 1,
    PermissionDenied = 2,
    AppNotFound = 3,
    StreamError = 4,
    Internal = 5,
}

impl ResultCode {
    fn from_raw(v: i32) -> Self {
        match v {
            0 => ResultCode::Ok,
            1 => ResultCode::OsVersion,
            2 => ResultCode::PermissionDenied,
            3 => ResultCode::AppNotFound,
            4 => ResultCode::StreamError,
            _ => ResultCode::Internal,
        }
    }

    fn into_error(self, context: &str) -> AppError {
        let msg = match self {
            ResultCode::Ok => return AppError::Stream(format!("{context}: unexpected Ok in error path")),
            ResultCode::OsVersion => "macOS 13.0+ required for ScreenCaptureKit",
            ResultCode::PermissionDenied => {
                "Screen Recording permission denied — enable it in System Settings → Privacy & Security → Screen Recording"
            }
            ResultCode::AppNotFound => "selected application is not running or has no audio",
            ResultCode::StreamError => "ScreenCaptureKit stream failed to start",
            ResultCode::Internal => "ScreenCaptureKit internal error (timeout)",
        };
        AppError::Stream(format!("{context}: {msg}"))
    }
}

type SampleCallback = extern "C" fn(
    user_data: *mut c_void,
    samples: *const f32,
    frames: i32,
    channels: i32,
);

extern "C" {
    fn ba_sck_create() -> *mut c_void;
    fn ba_sck_destroy(handle: *mut c_void);
    fn ba_sck_start_app(
        handle: *mut c_void,
        bundle_id: *const c_char,
        sample_rate: i32,
        channels: i32,
        callback: SampleCallback,
        user_data: *mut c_void,
    ) -> i32;
    fn ba_sck_start_system(
        handle: *mut c_void,
        exclude_current_app: i32,
        sample_rate: i32,
        channels: i32,
        callback: SampleCallback,
        user_data: *mut c_void,
    ) -> i32;
    /// 0 = clean stop; nonzero = Swift's 5 s `stopCapture` timeout fired and
    /// the dispatch queue may still call back, so do not free `user_data`.
    fn ba_sck_stop(handle: *mut c_void) -> i32;
}

pub struct SckCapture {
    handle: *mut c_void,
    state: Arc<CallbackState>,
}

unsafe impl Send for SckCapture {}

struct CallbackState {
    label: String,
    /// `UnsafeCell` (not `Mutex`) — SCK serial queue is the only mutator;
    /// avoids the kernel call `Mutex::lock` can take under priority inversion.
    bridge: UnsafeCell<BroadcastRx>,
    first_call_logged: AtomicBool,
    shutting_down: AtomicBool,
}

// SAFETY: `bridge` is only touched by `sample_trampoline` on the SCK
// serial queue; no other access after `start_*` returns.
unsafe impl Sync for CallbackState {}

extern "C" fn sample_trampoline(
    user_data: *mut c_void,
    samples: *const f32,
    frames: i32,
    channels: i32,
) {
    if user_data.is_null() || samples.is_null() || frames <= 0 || channels <= 0 {
        return;
    }
    let arc = unsafe { Arc::from_raw(user_data as *const CallbackState) };
    let state = Arc::clone(&arc);
    let _ = ManuallyDrop::new(arc);

    if state.shutting_down.load(Ordering::Acquire) {
        return;
    }
    if !state.first_call_logged.swap(true, Ordering::Relaxed) {
        info!(
            label = %state.label,
            frames,
            channels,
            "SCK: first audio buffer delivered"
        );
    }
    // SAFETY: see Sync impl on CallbackState.
    let bridge = unsafe { &mut *state.bridge.get() };
    bridge.apply_commands();
    let n = (frames as usize) * (channels as usize);
    let slice = unsafe { std::slice::from_raw_parts(samples, n) };
    bridge.broadcast(slice);
}

impl SckCapture {
    pub fn start_app(
        bundle_id: &str,
        sample_rate: u32,
        channels: u32,
        bridge: BroadcastRx,
    ) -> AppResult<Self> {
        let handle = unsafe { ba_sck_create() };
        if handle.is_null() {
            return Err(AppError::Stream("ScreenCaptureKit requires macOS 13.0+".into()));
        }

        let state = Arc::new(CallbackState {
            label: format!("app:{bundle_id}"),
            bridge: UnsafeCell::new(bridge),
            first_call_logged: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
        });
        let state_ptr = Arc::into_raw(Arc::clone(&state)) as *mut c_void;

        let bundle_cstr = CString::new(bundle_id)
            .map_err(|_| AppError::Validation("bundle id contains nul byte".into()))?;

        let code = unsafe {
            ba_sck_start_app(
                handle,
                bundle_cstr.as_ptr(),
                sample_rate as i32,
                channels as i32,
                sample_trampoline,
                state_ptr,
            )
        };
        let rc = ResultCode::from_raw(code);
        if rc != ResultCode::Ok {
            unsafe { ba_sck_destroy(handle) };
            drop(unsafe { Arc::from_raw(state_ptr as *const CallbackState) });
            return Err(rc.into_error(&format!("app audio capture ({bundle_id})")));
        }

        Ok(SckCapture { handle, state })
    }

    /// Start capturing system-wide audio. When `exclude_current_app` is true,
    /// our own process's audio is omitted from the mix (prevents feedback loops
    /// when System Audio is wired through Splitwave itself).
    pub fn start_system(
        exclude_current_app: bool,
        sample_rate: u32,
        channels: u32,
        bridge: BroadcastRx,
    ) -> AppResult<Self> {
        let handle = unsafe { ba_sck_create() };
        if handle.is_null() {
            return Err(AppError::Stream("ScreenCaptureKit requires macOS 13.0+".into()));
        }

        let state = Arc::new(CallbackState {
            label: "system".to_string(),
            bridge: UnsafeCell::new(bridge),
            first_call_logged: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
        });
        let state_ptr = Arc::into_raw(Arc::clone(&state)) as *mut c_void;

        let code = unsafe {
            ba_sck_start_system(
                handle,
                if exclude_current_app { 1 } else { 0 },
                sample_rate as i32,
                channels as i32,
                sample_trampoline,
                state_ptr,
            )
        };
        let rc = ResultCode::from_raw(code);
        if rc != ResultCode::Ok {
            unsafe { ba_sck_destroy(handle) };
            drop(unsafe { Arc::from_raw(state_ptr as *const CallbackState) });
            return Err(rc.into_error("system audio capture"));
        }

        Ok(SckCapture { handle, state })
    }
}

impl Drop for SckCapture {
    fn drop(&mut self) {
        self.state.shutting_down.store(true, Ordering::Release);
        let timed_out = unsafe { ba_sck_stop(self.handle) } != 0;
        unsafe { ba_sck_destroy(self.handle) };
        if timed_out {
            warn!(label = %self.state.label, "SCK stop timed out");
        }
    }
}
