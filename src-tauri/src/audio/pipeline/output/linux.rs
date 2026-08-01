use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::AppHandle;
use tracing::info;

use crate::error::AppResult;

use super::super::dag::OutputGraph;
use super::super::worker::WorkerCtrl;
use super::{spawn_speaker_worker, speaker_ring, SpeakerWorker};

pub(in crate::audio::pipeline) struct SpeakerResolved {
    pub node_id: String,
    pub sample_rate: u32,
    // PipeWire null-sink playback is stereo.
    pub out_channels: usize,
}

pub(in crate::audio::pipeline) struct SpeakerHandle {
    _playback: crate::audio::playback::Playback,
    _worker: SpeakerWorker,
}

pub(in crate::audio::pipeline) fn resolve_speaker(device_id: &str) -> AppResult<SpeakerResolved> {
    Ok(SpeakerResolved {
        node_id: device_id.to_string(),
        sample_rate: 48_000,
        out_channels: 2,
    })
}

pub(in crate::audio::pipeline) fn start_speaker_stream(
    _node_id: &str,
    spec: SpeakerResolved,
    graph: OutputGraph,
    meter: crate::audio::effects::MeterHandle,
    _app: &AppHandle,
) -> AppResult<(SpeakerHandle, WorkerCtrl, Arc<AtomicBool>)> {
    info!(node = %spec.node_id, sample_rate = spec.sample_rate, "opening speaker stream (PipeWire)");
    let dead = Arc::new(AtomicBool::new(false));

    let (producer, mut fill, level) = speaker_ring(spec.out_channels);
    let fill_pw = move |out: &mut [f32]| {
        fill(out, 0);
        out.len()
    };
    let playback = crate::audio::playback::Playback::start(&spec.node_id, fill_pw)?;

    let (worker_handle, ctrl) = spawn_speaker_worker(
        producer,
        level,
        spec.sample_rate,
        spec.out_channels,
        graph,
        meter,
    )?;
    Ok((
        SpeakerHandle { _playback: playback, _worker: worker_handle },
        ctrl,
        dead,
    ))
}
