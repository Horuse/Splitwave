use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::AppHandle;
use tracing::info;

use crate::error::AppResult;

use super::super::dag::OutputGraph;
use super::super::worker::WorkerCtrl;
use super::{spawn_speaker_worker, speaker_ring, SpeakerIo, SpeakerWorker, StreamGuard};

pub(in crate::audio::pipeline) struct SpeakerResolved {
    pub node_id: String,
    pub sample_rate: u32,
    // PipeWire null-sink playback is stereo.
    pub out_channels: usize,
}

pub(in crate::audio::pipeline) struct SpeakerHandle {
    _playback: crate::audio::playback::Playback,
    _worker: SpeakerWorker,
    _alive: StreamGuard,
}

pub(in crate::audio::pipeline) fn resolve_speaker(device_id: &str) -> AppResult<SpeakerResolved> {
    let node = crate::audio::pw_enum::nodes_by_class("Audio/Sink")?
        .into_iter()
        .find(|node| node.name == device_id)
        .ok_or_else(|| {
            crate::error::AppError::Device(format!("PipeWire sink not found: {device_id}"))
        })?;
    // PipeWire exposes the sink's negotiated graph clock as audio.rate. Do
    // not silently pretend it is 48 kHz: this feeds the explicit engine→sink
    // resampler and speaker fill clock.
    let sample_rate = node.sample_rate.ok_or_else(|| {
        crate::error::AppError::Device(format!(
            "PipeWire sink {device_id:?} did not report audio.rate"
        ))
    })?;
    Ok(SpeakerResolved {
        node_id: node.name,
        sample_rate,
        out_channels: 2,
    })
}

pub(in crate::audio::pipeline) fn start_speaker_stream(
    _node_id: &str,
    spec: SpeakerResolved,
    graph: OutputGraph,
    meter: crate::audio::effects::MeterHandle,
    _app: &AppHandle,
) -> AppResult<(SpeakerHandle, WorkerCtrl, Arc<AtomicBool>, SpeakerIo)> {
    info!(node = %spec.node_id, sample_rate = spec.sample_rate, "opening speaker stream (PipeWire)");
    let dead = Arc::new(AtomicBool::new(false));

    let (producer, mut fill, level, target, io) =
        speaker_ring(spec.out_channels, spec.sample_rate, graph.latency_frames());
    let fill_pw = move |out: &mut [f32]| {
        fill(out, 0);
        out.len()
    };
    let playback = crate::audio::playback::Playback::start(&spec.node_id, fill_pw)?;

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
        SpeakerHandle {
            _playback: playback,
            _worker: worker_handle,
            _alive: StreamGuard::new(),
        },
        ctrl,
        dead,
        io,
    ))
}
