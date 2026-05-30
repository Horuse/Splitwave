use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rtrb::RingBuffer;
use tauri::AppHandle;
use tracing::info;

use crate::audio::streams;
use crate::error::AppResult;

use super::super::dag::OutputGraph;
use super::super::worker::WorkerCtrl;
use super::{spawn_speaker_worker, SpeakerWorker, SPEAKER_RING_CAPACITY};

pub(in crate::audio::pipeline) struct SpeakerResolved {
    pub node_id: String,
    pub sample_rate: u32,
}

pub(in crate::audio::pipeline) struct SpeakerHandle {
    _playback: crate::audio::playback::Playback,
    _worker: SpeakerWorker,
}

pub(in crate::audio::pipeline) fn resolve_speaker(device_id: &str) -> AppResult<SpeakerResolved> {
    Ok(SpeakerResolved {
        node_id: device_id.to_string(),
        sample_rate: 48_000,
    })
}

pub(in crate::audio::pipeline) fn start_speaker_stream(
    _node_id: &str,
    spec: SpeakerResolved,
    graph: OutputGraph,
    _app: &AppHandle,
) -> AppResult<(SpeakerHandle, WorkerCtrl, Arc<AtomicBool>)> {
    info!(node = %spec.node_id, sample_rate = spec.sample_rate, "opening speaker stream (PipeWire)");
    let dead = Arc::new(AtomicBool::new(false));

    let (producer, mut consumer) = RingBuffer::<f32>::new(SPEAKER_RING_CAPACITY);
    let fill = move |out: &mut [f32]| {
        streams::bulk_pop(&mut consumer, out);
        out.len()
    };
    let playback = crate::audio::playback::Playback::start(&spec.node_id, fill)?;

    let (worker_handle, ctrl) = spawn_speaker_worker(producer, spec.sample_rate, graph)?;
    Ok((
        SpeakerHandle { _playback: playback, _worker: worker_handle },
        ctrl,
        dead,
    ))
}
