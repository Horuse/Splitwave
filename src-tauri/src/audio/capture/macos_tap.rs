//! Rust binding for the Swift `CATapCapture` static library (native/).
//!
//! Core Audio process taps scope capture to process objects at the HAL, so a
//! per-app tap cannot pick up another app's audio, and only the System Audio
//! Recording permission is required. Available on macOS 14.4+; older hosts use
//! the ScreenCaptureKit path in `macos.rs`.

use std::cell::UnsafeCell;
use std::ffi::{c_void, CString};
use std::mem::ManuallyDrop;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tracing::info;

use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultCode {
    Ok = 0,
    OsVersion = 1,
    PermissionDenied = 2,
    AppNotFound = 3,
    TapError = 4,
    Internal = 5,
}

impl ResultCode {
    fn from_raw(v: i32) -> Self {
        match v {
            0 => ResultCode::Ok,
            1 => ResultCode::OsVersion,
            2 => ResultCode::PermissionDenied,
            3 => ResultCode::AppNotFound,
            4 => ResultCode::TapError,
            _ => ResultCode::Internal,
        }
    }

    fn into_error(self, context: &str) -> AppError {
        let msg = match self {
            ResultCode::Ok => {
                return AppError::Stream(format!("{context}: unexpected Ok in error path"))
            }
            ResultCode::OsVersion => "macOS 14.4+ required for Core Audio process taps",
            ResultCode::PermissionDenied => {
                "System Audio Recording permission denied — enable it in System Settings → Privacy & Security → System Audio Recording"
            }
            ResultCode::AppNotFound => {
                "selected application has no audio process — browser web apps render their audio through the browser itself, so capture the browser instead"
            }
            ResultCode::TapError => "process tap or aggregate device failed to start",
            ResultCode::Internal => "process tap internal error",
        };
        AppError::Stream(format!("{context}: {msg}"))
    }
}

type SampleCallback =
    extern "C" fn(user_data: *mut c_void, samples: *const f32, frames: i32, channels: i32);

extern "C" {
    fn ba_tap_available() -> i32;
    fn ba_tap_default_rate() -> f64;
    fn ba_tap_create() -> *mut c_void;
    fn ba_tap_destroy(handle: *mut c_void);
    fn ba_tap_start_app(
        handle: *mut c_void,
        bundle_id: *const c_char,
        callback: SampleCallback,
        user_data: *mut c_void,
    ) -> i32;
    fn ba_tap_start_system(
        handle: *mut c_void,
        exclude_current_app: i32,
        callback: SampleCallback,
        user_data: *mut c_void,
    ) -> i32;
    fn ba_tap_format(handle: *mut c_void, sample_rate: *mut f64, channels: *mut i32) -> i32;
    fn ba_tap_stop(handle: *mut c_void);
}

pub fn available() -> bool {
    (unsafe { ba_tap_available() }) != 0
}

/// Rate the tap will run at, read from the default output device before the
/// graph is built. Zero when no output device is present.
pub fn default_rate() -> u32 {
    (unsafe { ba_tap_default_rate() }) as u32
}

pub struct TapCapture {
    handle: *mut c_void,
    state: Arc<CallbackState>,
    channels: u32,
    sample_rate: u32,
}

/// Non-owning format probe used only by the normalizer thread. `TapCapture`
/// outlives that thread (NormalizedInput joins it before dropping capture), so
/// the native handle remains valid for every query.
#[derive(Clone, Copy)]
pub struct TapRateProbe {
    handle: *mut c_void,
}

unsafe impl Send for TapRateProbe {}

unsafe impl Send for TapCapture {}

struct CallbackState {
    label: String,
    /// `UnsafeCell` (not `Mutex`) — the IOProc queue is the only mutator.
    bridge: UnsafeCell<BroadcastRx>,
    first_call_logged: AtomicBool,
    shutting_down: AtomicBool,
}

// SAFETY: `bridge` is only touched by `sample_trampoline` on the tap's serial
// IOProc queue; no other access after `start_*` returns.
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
        info!(label = %state.label, frames, channels, "tap: first audio buffer delivered");
    }
    // SAFETY: see Sync impl on CallbackState.
    let bridge = unsafe { &mut *state.bridge.get() };
    bridge.apply_commands();
    let n = (frames as usize) * (channels as usize);
    let slice = unsafe { std::slice::from_raw_parts(samples, n) };
    bridge.broadcast(slice);
}

impl TapCapture {
    pub fn start_app(bundle_id: &str, bridge: BroadcastRx) -> AppResult<Self> {
        let bundle_cstr = CString::new(bundle_id)
            .map_err(|_| AppError::Validation("bundle id contains nul byte".into()))?;
        Self::start(
            format!("app:{bundle_id}"),
            &format!("app audio capture ({bundle_id})"),
            bridge,
            |handle, callback, user_data| unsafe {
                ba_tap_start_app(handle, bundle_cstr.as_ptr(), callback, user_data)
            },
        )
    }

    /// When `exclude_current_app` is set our own output is dropped from the
    /// mix, which prevents a feedback loop when System Audio is routed back
    /// through Splitwave.
    pub fn start_system(exclude_current_app: bool, bridge: BroadcastRx) -> AppResult<Self> {
        Self::start(
            "system".to_string(),
            "system audio capture",
            bridge,
            |handle, callback, user_data| unsafe {
                ba_tap_start_system(
                    handle,
                    if exclude_current_app { 1 } else { 0 },
                    callback,
                    user_data,
                )
            },
        )
    }

    fn start(
        label: String,
        context: &str,
        bridge: BroadcastRx,
        launch: impl FnOnce(*mut c_void, SampleCallback, *mut c_void) -> i32,
    ) -> AppResult<Self> {
        let handle = unsafe { ba_tap_create() };
        if handle.is_null() {
            return Err(AppError::Stream(
                "Core Audio process taps require macOS 14.4+".into(),
            ));
        }

        let state = Arc::new(CallbackState {
            label,
            bridge: UnsafeCell::new(bridge),
            first_call_logged: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
        });
        let state_ptr = Arc::into_raw(Arc::clone(&state)) as *mut c_void;

        let rc = ResultCode::from_raw(launch(handle, sample_trampoline, state_ptr));
        if rc != ResultCode::Ok {
            unsafe { ba_tap_destroy(handle) };
            drop(unsafe { Arc::from_raw(state_ptr as *const CallbackState) });
            return Err(rc.into_error(context));
        }

        let mut sample_rate = 0.0f64;
        let mut channels = 0i32;
        let format_rc =
            ResultCode::from_raw(unsafe { ba_tap_format(handle, &mut sample_rate, &mut channels) });
        if format_rc != ResultCode::Ok {
            unsafe { ba_tap_stop(handle) };
            unsafe { ba_tap_destroy(handle) };
            drop(unsafe { Arc::from_raw(state_ptr as *const CallbackState) });
            return Err(format_rc.into_error(context));
        }

        Ok(TapCapture {
            handle,
            state,
            channels: channels as u32,
            sample_rate: sample_rate as u32,
        })
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn rate_probe(&self) -> TapRateProbe {
        TapRateProbe {
            handle: self.handle,
        }
    }
}

impl TapRateProbe {
    pub fn sample_rate(&self) -> Option<u32> {
        let mut sample_rate = 0.0f64;
        let mut channels = 0i32;
        let rc = ResultCode::from_raw(unsafe {
            ba_tap_format(self.handle, &mut sample_rate, &mut channels)
        });
        (rc == ResultCode::Ok && sample_rate > 0.0 && channels > 0).then_some(sample_rate as u32)
    }
}

impl Drop for TapCapture {
    fn drop(&mut self) {
        self.state.shutting_down.store(true, Ordering::Release);
        unsafe { ba_tap_stop(self.handle) };
        unsafe { ba_tap_destroy(self.handle) };
    }
}
