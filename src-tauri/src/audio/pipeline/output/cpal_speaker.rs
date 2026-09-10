use cpal::traits::StreamTrait;
use tracing::warn;

use crate::audio::device::{self, DeviceKind};
use crate::audio::pipeline::native::native_config;
use crate::error::AppResult;

use super::{SpeakerWorker, StreamGuard};

pub(in crate::audio::pipeline) struct SpeakerResolved {
    pub device: cpal::Device,
    pub config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
    pub out_channels: usize,
    pub sample_rate: u32,
}

/// Field order: the stream drops before the worker so the audio callback stops
/// before the ring is freed.
pub(in crate::audio::pipeline) struct SpeakerHandle {
    _stream: cpal::Stream,
    _worker: SpeakerWorker,
    _alive: StreamGuard,
}

impl SpeakerHandle {
    pub(super) fn new(stream: cpal::Stream, worker: SpeakerWorker, alive: StreamGuard) -> Self {
        Self {
            _stream: stream,
            _worker: worker,
            _alive: alive,
        }
    }
}

// cpal's coreaudio backend registers a device-alive property listener for any
// non-default device (which `device::find` always returns) whose callback closure
// holds another clone of the `Stream`'s inner `Arc`. That's a permanent reference
// cycle: dropping our `_stream` handle alone never reaches refcount zero, so the
// AudioUnit is never disposed and keeps calling `fill` on a ring nobody drains.
// `pause` reaches the device through `&self` and stops it for real. WASAPI
// benefits from the same explicit pause on teardown.
impl Drop for SpeakerHandle {
    fn drop(&mut self) {
        if let Err(e) = self._stream.pause() {
            warn!(error = %e, "failed to pause speaker stream on teardown");
        }
    }
}

pub(in crate::audio::pipeline) fn resolve_speaker(device_id: &str) -> AppResult<SpeakerResolved> {
    let device = device::find(DeviceKind::Output, device_id)?;
    let native = native_config(DeviceKind::Output, &device, device_id)?;
    Ok(SpeakerResolved {
        device,
        config: native.config,
        sample_format: native.sample_format,
        out_channels: native.channels as usize,
        sample_rate: native.sample_rate,
    })
}
