use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cpal::traits::StreamTrait;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{error, info, warn};

use crate::audio::device::{self, DeviceKind};
use crate::audio::health;
use crate::audio::streams;
use crate::error::AppResult;

use super::super::dag::OutputGraph;
use super::super::native::native_config;
use super::super::worker::WorkerCtrl;
use super::{speaker_callback, speaker_renderer, SpeakerIo, SpeakerLink, StreamGuard};

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

// `Stream::drop` isn't guaranteed to stop the underlying device (cpal's macOS
// backend never does for a non-default device, see the macOS SpeakerHandle);
// call `pause` explicitly so teardown doesn't depend on that guarantee here too.
// The renderer is taken back first so the graph drops on this thread.
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
        "opening speaker stream (WASAPI)",
    );

    // Built before the stream opens: a renderer that cannot be built must not
    // leave behind a stream nothing pauses.
    let (renderer, ctrl, io) =
        speaker_renderer(graph, spec.sample_rate, spec.out_channels, None, meter)?;

    let dead = Arc::new(AtomicBool::new(false));

    let (mut link, fill) = speaker_callback();
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

    let stream = streams::build_output_stream(
        &spec.device,
        &spec.config,
        spec.sample_format,
        spec.out_channels,
        fill,
        err_cb,
    )?;
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
