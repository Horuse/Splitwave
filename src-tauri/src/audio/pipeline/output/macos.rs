use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use cpal::traits::DeviceTrait;
use rtrb::RingBuffer;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::audio::device::{self, DeviceKind};
use crate::audio::streams;
use crate::error::{AppError, AppResult};

use super::super::dag::OutputGraph;
use super::super::native::native_config;
use super::super::worker::WorkerCtrl;
use super::{spawn_speaker_worker, SpeakerWorker, SPEAKER_RING_CAPACITY};

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

// Field order: the stream drops before the worker so the audio callback stops
// before the ring is freed.
pub(in crate::audio::pipeline) struct SpeakerHandle {
    _stream: cpal::Stream,
    _worker: SpeakerWorker,
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

// Substring match on cpal's stable Display -- AppError flattens the variant.
fn is_device_not_available(e: &AppError) -> bool {
    matches!(e, AppError::Stream(s) if s.contains("no longer available"))
}

pub(in crate::audio::pipeline) fn start_speaker_stream(
    node_id: &str,
    spec: SpeakerResolved,
    graph: OutputGraph,
    app: &AppHandle,
) -> AppResult<(SpeakerHandle, WorkerCtrl, Arc<AtomicBool>)> {
    let device_name = spec
        .device
        .name()
        .unwrap_or_else(|_| "<unknown>".into());
    info!(
        device = %device_name,
        sample_rate = spec.sample_rate,
        channels = spec.out_channels,
        format = ?spec.sample_format,
        "opening speaker stream",
    );

    // AirPods A2DP/HFP switch can race resolve_output; verify state fresh.
    {
        let fresh = crate::audio::macos_hal::find_output_device(&device_name);
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

    let dead = Arc::new(AtomicBool::new(false));

    let mut producer_holder: Option<rtrb::Producer<f32>> = None;
    let mut stream_holder: Option<cpal::Stream> = None;
    for attempt in 1..=SPEAKER_MAX_ATTEMPTS {
        let (producer, mut consumer) = RingBuffer::<f32>::new(SPEAKER_RING_CAPACITY);
        let fill = move |stereo_out: &mut [f32], _frames: usize| {
            streams::bulk_pop(&mut consumer, stereo_out);
        };
        let app_err = app.clone();
        let dead_cb = dead.clone();
        let node_id_cb = node_id.to_string();
        let err_cb = move |_e: cpal::StreamError| {
            dead_cb.store(true, Ordering::Relaxed);
            let _ = app_err.emit("audio://speaker_error", json!({ "nodeId": node_id_cb }));
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
                producer_holder = Some(producer);
                stream_holder = Some(s);
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
    let producer = producer_holder.expect("loop sets producer on success or returns Err");
    let stream = stream_holder.expect("loop sets stream on success or returns Err");

    let (worker_handle, ctrl) = spawn_speaker_worker(producer, spec.sample_rate, graph)?;
    Ok((
        SpeakerHandle { _stream: stream, _worker: worker_handle },
        ctrl,
        dead,
    ))
}
