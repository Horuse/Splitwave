use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cpal::traits::StreamTrait;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{error, info, warn};

use crate::audio::device::{self, DeviceKind};
use crate::audio::health;
use crate::audio::macos_hal;
use crate::audio::streams;
use crate::error::{AppError, AppResult};

use super::super::dag::OutputGraph;
use super::super::native::native_config;
use super::super::worker::WorkerCtrl;
use super::{
    device_block, speaker_callback, speaker_renderer, SpeakerIo, SpeakerLink, StreamGuard,
};

// Bluetooth AUHAL often returns DeviceNotAvailable on first bind; retry covers settling.
const SPEAKER_MAX_ATTEMPTS: u32 = 3;
const SPEAKER_RETRY_DELAY: Duration = Duration::from_millis(300);

pub(in crate::audio::pipeline) struct SpeakerResolved {
    pub device: cpal::Device,
    pub config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
    pub out_channels: usize,
    pub sample_rate: u32,
}

pub(in crate::audio::pipeline) struct SpeakerHandle {
    _stream: cpal::Stream,
    link: SpeakerLink,
    _alive: StreamGuard,
}

// cpal's coreaudio backend registers a device-alive property listener for any
// non-default device (which `device::find` always returns -- see its comment)
// whose callback closure holds another clone of the `Stream`'s inner `Arc`.
// That's a permanent reference cycle: dropping our `_stream` handle alone
// never reaches refcount zero, so the AudioUnit is never disposed and keeps
// calling `fill` on a ring nobody drains anymore. `pause` reaches the
// AudioUnit through `&self` and stops it for real, independent of the cycle.
//
// The renderer (and the graph it owns) is taken back first, so it drops here
// rather than leaking with that closure.
impl Drop for SpeakerHandle {
    fn drop(&mut self) {
        let renderer = self.link.retire();
        if let Err(e) = self._stream.pause() {
            warn!(error = %e, "failed to pause speaker stream on teardown");
        }
        drop(renderer);
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

fn is_device_not_available(e: &AppError) -> bool {
    matches!(e, AppError::DeviceUnavailable(_))
}

pub(in crate::audio::pipeline) fn start_speaker_stream(
    node_id: &str,
    spec: SpeakerResolved,
    graph: OutputGraph,
    meter: crate::audio::effects::MeterHandle,
    app: &AppHandle,
) -> AppResult<(SpeakerHandle, WorkerCtrl, Arc<AtomicBool>, SpeakerIo)> {
    let device_name =
        crate::audio::device::cpal_name(&spec.device).unwrap_or_else(|| "<unknown>".into());
    info!(
        device = %device_name,
        sample_rate = spec.sample_rate,
        channels = spec.out_channels,
        format = ?spec.sample_format,
        "opening speaker stream",
    );

    // AirPods A2DP/HFP switch can race resolve_output; verify state fresh.
    {
        let fresh = macos_hal::find_output_device(&device_name);
        match fresh {
            None => warn!(device = %device_name, "HAL no longer sees the device"),
            Some(hal) if hal.sample_rate != spec.sample_rate => warn!(
                device = %device_name,
                resolved_sample_rate = spec.sample_rate,
                current_sample_rate = hal.sample_rate,
                "device sample rate changed between resolve and open"
            ),
            Some(hal) if hal.channels as usize != spec.out_channels => warn!(
                device = %device_name,
                resolved_channels = spec.out_channels,
                current_channels = hal.channels,
                "device channel count changed between resolve and open"
            ),
            Some(_) => {}
        }
    }

    // One callback per engine block. The device may clamp the request to its
    // range; the renderer adapts to whatever it grants.
    let requested = device_block(graph.block_frames(), graph.sample_rate(), spec.sample_rate);
    let granted = macos_hal::set_buffer_frames(DeviceKind::Output, &device_name, requested);
    info!(device = %device_name, requested, granted, "speaker buffer size");

    let dead = Arc::new(AtomicBool::new(false));

    let mut opened: Option<(cpal::Stream, SpeakerLink)> = None;
    for attempt in 1..=SPEAKER_MAX_ATTEMPTS {
        let (link, fill) = speaker_callback();
        let app_err = app.clone();
        let dead_cb = dead.clone();
        let node_id_cb = node_id.to_string();
        let err_cb = move |e: cpal::Error| {
            if dead_cb.swap(true, Ordering::Relaxed) {
                return;
            }
            health::bump(&health::STREAM_ERRORS, 1);
            error!(node_id = %node_id_cb, error = %e, "speaker stream error");
            let _ = app_err.emit(
                "audio://speaker_error",
                json!({ "nodeId": node_id_cb, "error": format!("{e}") }),
            );
        };
        match streams::build_output_stream(
            &spec.device,
            &spec.config,
            spec.sample_format,
            spec.out_channels,
            fill,
            err_cb,
        ) {
            Ok(s) => {
                opened = Some((s, link));
                break;
            }
            Err(e) if attempt < SPEAKER_MAX_ATTEMPTS && is_device_not_available(&e) => {
                warn!(
                    attempt,
                    error = %e,
                    "DeviceNotAvailable from cpal; retrying after delay"
                );
                thread::sleep(SPEAKER_RETRY_DELAY);
            }
            Err(e) => return Err(e),
        }
    }
    let (stream, mut link) = opened.expect("loop opens the stream or returns Err");

    let hardware =
        macos_hal::io_latency(DeviceKind::Output, &device_name).map(|l| l.hardware_frames);
    let (renderer, ctrl, io) =
        speaker_renderer(graph, spec.sample_rate, spec.out_channels, hardware, meter)?;
    link.attach(renderer);
    Ok((
        SpeakerHandle {
            _stream: stream,
            link,
            _alive: StreamGuard::new(),
        },
        ctrl,
        dead,
        io,
    ))
}
