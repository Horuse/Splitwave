use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::AppHandle;
use tracing::info;

use crate::error::AppResult;

use super::super::dag::OutputGraph;
use super::super::worker::WorkerCtrl;
use super::{speaker_callback, speaker_renderer, SpeakerIo, SpeakerLink, StreamGuard};

pub(in crate::audio::pipeline) struct SpeakerResolved {
    pub node_id: String,
    pub sample_rate: u32,
    pub out_channels: usize,
}

pub(in crate::audio::pipeline) struct SpeakerHandle {
    _playback: crate::audio::playback::Playback,
    link: SpeakerLink,
    _alive: StreamGuard,
}

// The renderer is taken back before the playback thread stops, so the graph
// drops on this thread rather than inside the PipeWire process callback.
impl Drop for SpeakerHandle {
    fn drop(&mut self) {
        drop(self.link.retire());
    }
}

pub(in crate::audio::pipeline) fn resolve_speaker(device_id: &str) -> AppResult<SpeakerResolved> {
    let info =
        crate::audio::device::device_info(crate::audio::device::DeviceKind::Output, device_id)?;
    Ok(SpeakerResolved {
        node_id: device_id.to_string(),
        sample_rate: info.sample_rate,
        out_channels: usize::from(info.channels),
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
    // Built before playback starts: a renderer that cannot be built must not
    // leave a PipeWire stream running.
    let block_frames = graph.block_frames();
    let (renderer, ctrl, io) =
        speaker_renderer(graph, spec.sample_rate, spec.out_channels, None, meter)?;
    let dead = Arc::new(AtomicBool::new(false));

    let (mut link, mut fill) = speaker_callback();
    let channels = spec.out_channels.max(1);
    let fill_pw = move |out: &mut [f32]| {
        fill(out, out.len() / channels);
        out.len()
    };
    let playback = crate::audio::playback::Playback::start(
        &spec.node_id,
        spec.sample_rate,
        spec.out_channels,
        block_frames,
        fill_pw,
    )?;
    link.attach(renderer);
    Ok((
        SpeakerHandle {
            _playback: playback,
            link,
            _alive: StreamGuard::new(),
        },
        ctrl,
        dead,
        io,
    ))
}
