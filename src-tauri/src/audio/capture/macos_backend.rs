//! Picks the macOS capture backend once per stream.
//!
//! Core Audio process taps (14.4+) are the correct path: per-process scoping at
//! the HAL and no Screen Recording permission. ScreenCaptureKit remains for
//! 13.0-14.3, where it scopes audio by display content and cannot keep other
//! apps out of the mix.

use tracing::info;

use crate::audio::input_bridge::BroadcastRx;
use crate::error::AppResult;

use super::macos::SckCapture;
use super::macos_tap::{self, TapCapture};

/// ScreenCaptureKit is configured for interleaved stereo; taps report their own
/// format.
const SCK_CHANNELS: u32 = 2;
const SCK_RATE: u32 = 48_000;

pub enum Capture {
    Tap(TapCapture),
    /// Held only to keep the stream alive until drop; its format is fixed.
    Sck(#[allow(dead_code)] SckCapture),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Tap,
    Sck,
}

pub fn uses_taps() -> bool {
    backend() == Backend::Tap
}

fn backend() -> Backend {
    if macos_tap::available() {
        Backend::Tap
    } else {
        Backend::Sck
    }
}

/// Rate the capture will deliver, needed before the graph is built. Taps follow
/// the default output device.
pub fn capture_rate() -> u32 {
    match backend() {
        Backend::Tap => match macos_tap::default_rate() {
            0 => SCK_RATE,
            rate => rate,
        },
        Backend::Sck => SCK_RATE,
    }
}

impl Capture {
    pub fn start_app(bundle_id: &str, sample_rate: u32, bridge: BroadcastRx) -> AppResult<Self> {
        match backend() {
            Backend::Tap => {
                info!(%bundle_id, "starting app-audio capture (Core Audio process tap)");
                Ok(Capture::Tap(TapCapture::start_app(bundle_id, bridge)?))
            }
            Backend::Sck => {
                info!(%bundle_id, "starting app-audio capture (ScreenCaptureKit, macOS < 14.4)");
                Ok(Capture::Sck(SckCapture::start_app(
                    bundle_id,
                    sample_rate,
                    SCK_CHANNELS,
                    bridge,
                )?))
            }
        }
    }

    pub fn start_system(
        exclude_current_app: bool,
        sample_rate: u32,
        bridge: BroadcastRx,
    ) -> AppResult<Self> {
        match backend() {
            Backend::Tap => {
                info!(
                    exclude_current_app,
                    "starting system-audio capture (Core Audio process tap)"
                );
                Ok(Capture::Tap(TapCapture::start_system(
                    exclude_current_app,
                    bridge,
                )?))
            }
            Backend::Sck => {
                info!(
                    exclude_current_app,
                    "starting system-audio capture (ScreenCaptureKit, macOS < 14.4)"
                );
                Ok(Capture::Sck(SckCapture::start_system(
                    exclude_current_app,
                    sample_rate,
                    SCK_CHANNELS,
                    bridge,
                )?))
            }
        }
    }

    pub fn channels(&self) -> u32 {
        match self {
            Capture::Tap(tap) => tap.channels(),
            Capture::Sck(_) => SCK_CHANNELS,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        match self {
            Capture::Tap(tap) => tap.sample_rate(),
            Capture::Sck(_) => SCK_RATE,
        }
    }

    pub fn tap_rate_probe(&self) -> Option<macos_tap::TapRateProbe> {
        match self {
            Capture::Tap(tap) => Some(tap.rate_probe()),
            Capture::Sck(_) => None,
        }
    }
}
