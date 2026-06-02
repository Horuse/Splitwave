use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cpal::traits::DeviceTrait;
use rtrb::RingBuffer;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::info;

use crate::audio::device::{self, DeviceKind};
use crate::audio::streams;
use crate::error::AppResult;

use super::super::dag::OutputGraph;
use super::super::native::native_config;
use super::super::worker::WorkerCtrl;
use super::{spawn_speaker_worker, SpeakerWorker, SPEAKER_RING_CAPACITY};

pub(in crate::audio::pipeline) struct SpeakerResolved {
    pub device: cpal::Device,
    pub config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
    pub out_channels: usize,
    pub sample_rate: u32,
}

// The stream drops before the worker so the audio callback stops before the
// ring is freed.
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

pub(in crate::audio::pipeline) fn start_speaker_stream(
    node_id: &str,
    spec: SpeakerResolved,
    graph: OutputGraph,
    app: &AppHandle,
) -> AppResult<(SpeakerHandle, WorkerCtrl, Arc<AtomicBool>)> {
    let device_name = spec.device.name().unwrap_or_else(|_| "<unknown>".into());
    info!(
        device = %device_name,
        sample_rate = spec.sample_rate,
        channels = spec.out_channels,
        format = ?spec.sample_format,
        "opening speaker stream (WASAPI)",
    );

    let dead = Arc::new(AtomicBool::new(false));

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

    let stream = streams::build_output_stream(
        &spec.device,
        &spec.config,
        spec.sample_format,
        spec.out_channels,
        fill,
        err_cb,
    )?;

    let (worker_handle, ctrl) = spawn_speaker_worker(producer, spec.sample_rate, graph)?;
    Ok((
        SpeakerHandle {
            _stream: stream,
            _worker: worker_handle,
        },
        ctrl,
        dead,
    ))
}
