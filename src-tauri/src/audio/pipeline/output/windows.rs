use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cpal::traits::DeviceTrait;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{error, info};

use crate::audio::health;
use crate::audio::streams;
use crate::error::AppResult;

use super::super::dag::OutputGraph;
use super::super::worker::WorkerCtrl;
use super::{spawn_speaker_worker, speaker_ring, SpeakerIo};
pub(in crate::audio::pipeline) use super::cpal_speaker::{resolve_speaker, SpeakerHandle, SpeakerResolved};

pub(in crate::audio::pipeline) fn start_speaker_stream(
    node_id: &str,
    spec: SpeakerResolved,
    graph: OutputGraph,
    meter: crate::audio::effects::MeterHandle,
    app: &AppHandle,
) -> AppResult<(SpeakerHandle, WorkerCtrl, Arc<AtomicBool>, SpeakerIo)> {
    let device_name = spec.device.name().unwrap_or_else(|_| "<unknown>".into());
    info!(
        device = %device_name,
        sample_rate = spec.sample_rate,
        channels = spec.out_channels,
        format = ?spec.sample_format,
        "opening speaker stream (WASAPI)",
    );

    let dead = Arc::new(AtomicBool::new(false));

    let (producer, fill, level, target, io) = speaker_ring(
        spec.out_channels,
        graph.sample_rate(),
        spec.sample_rate,
        graph.latency_frames(),
    );
    let app_err = app.clone();
    let dead_cb = dead.clone();
    let node_id_cb = node_id.to_string();
    let err_cb = move |e: cpal::StreamError| {
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

    let stream = streams::build_output_stream(
        &spec.device,
        &spec.config,
        spec.sample_format,
        spec.out_channels,
        fill,
        err_cb,
    )?;

    let (worker_handle, ctrl) = spawn_speaker_worker(
        producer,
        level,
        target,
        io.sample_rate.clone(),
        spec.out_channels,
        graph,
        meter,
    )?;
    Ok((
        SpeakerHandle::new(stream, worker_handle, super::StreamGuard::new()),
        ctrl,
        dead,
        io,
    ))
}
