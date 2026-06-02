use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rtrb::Producer;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::warn;

use crate::audio::clock::{ClockSource, SystemClockTicker};
use crate::audio::encoders::{build_encoder, AudioEncoder};
use crate::audio::graph::{OutputSpec, RecordingFormat, ValidOutput};
use crate::audio::streams;
use crate::error::{AppError, AppResult};

use super::dag::{OutputGraph, DSP_BLOCK_FRAMES};
use super::worker::{dsp_worker, WorkerCtrl, WorkerPacing};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

pub(super) use platform::{start_speaker_stream, SpeakerHandle, SpeakerResolved};

// No live inputs -> fall back to 48 kHz for the recorder.
const RECORDER_DEFAULT_SR: u32 = 48_000;

// 32k f32 samples = ~340 ms @ 48 kHz stereo; absorbs cpal/scheduler jitter.
pub(super) const SPEAKER_RING_CAPACITY: usize = 32_768;

pub(super) enum ResolvedOutput {
    Speaker(SpeakerResolved),
    File {
        path: PathBuf,
        sample_rate: u32,
        format: RecordingFormat,
    },
}

impl ResolvedOutput {
    pub(super) fn sample_rate(&self) -> u32 {
        match self {
            ResolvedOutput::Speaker(s) => s.sample_rate,
            ResolvedOutput::File { sample_rate, .. } => *sample_rate,
        }
    }
}

pub(super) fn resolve_output(
    out: &ValidOutput,
    file_sr_hint: Option<u32>,
) -> AppResult<ResolvedOutput> {
    match &out.spec {
        OutputSpec::Speaker { device_id } => {
            Ok(ResolvedOutput::Speaker(platform::resolve_speaker(device_id)?))
        }
        OutputSpec::FileRecording { file_path, format } => Ok(ResolvedOutput::File {
            path: PathBuf::from(file_path),
            sample_rate: file_sr_hint.unwrap_or(RECORDER_DEFAULT_SR),
            format: *format,
        }),
    }
}

pub(super) struct SpeakerWorker {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for SpeakerWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub(super) struct RecorderWorker {
    pub stop: Arc<AtomicBool>,
    pub join: Option<JoinHandle<()>>,
}

impl Drop for RecorderWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

// Shared by both platforms' `start_speaker_stream`: a Clock-paced worker that
// mixes the output sub-graph and bulk-pushes blocks into the speaker ring.
pub(super) fn spawn_speaker_worker(
    mut producer: Producer<f32>,
    sample_rate: u32,
    graph: OutputGraph,
) -> AppResult<(SpeakerWorker, WorkerCtrl)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (worker, ctrl) = dsp_worker(graph);
    let clock: Box<dyn ClockSource> =
        Box::new(SystemClockTicker::new(sample_rate, DSP_BLOCK_FRAMES));
    let pacing = WorkerPacing::Clock(clock);
    let join = thread::Builder::new()
        .name(format!("speaker:{sample_rate}"))
        .spawn(move || {
            worker.run(stop_thread, pacing, |block| {
                streams::bulk_push(&mut producer, block);
                Ok(())
            });
        })
        .map_err(|e| AppError::Stream(format!("spawn speaker worker: {e}")))?;
    Ok((SpeakerWorker { stop, join: Some(join) }, ctrl))
}

// Drives analyzers when there's no real output; sink discards the mix.
pub(super) fn start_monitor_worker(
    graph: OutputGraph,
) -> AppResult<(RecorderWorker, WorkerCtrl)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (worker, ctrl) = dsp_worker(graph);
    let pacing = WorkerPacing::OnAvailability;
    let join = thread::Builder::new()
        .name("monitor".into())
        .spawn(move || {
            worker.run(stop_thread, pacing, |_block| Ok(()));
        })
        .map_err(|e| AppError::Stream(format!("spawn monitor worker: {e}")))?;
    Ok((
        RecorderWorker {
            stop,
            join: Some(join),
        },
        ctrl,
    ))
}

pub(super) fn start_recorder_worker(
    node_id: String,
    path: PathBuf,
    sample_rate: u32,
    format: RecordingFormat,
    graph: OutputGraph,
    app: AppHandle,
) -> AppResult<(RecorderWorker, WorkerCtrl)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (worker, ctrl) = dsp_worker(graph);
    let pacing = WorkerPacing::OnAvailability;

    let join = thread::Builder::new()
        .name(format!("recorder:{}", path.display()))
        .spawn(move || {
            // Inside the worker thread so slow encoder init (libopus,
            // libmp3lame, AVAudioFile) doesn't stagger recorder starts.
            let encoder: Box<dyn AudioEncoder> =
                match build_encoder(&path, sample_rate, format) {
                    Ok(e) => e,
                    Err(e) => {
                        warn!(node = %node_id, error = %e, "recorder init failed");
                        let _ = app.emit(
                            "audio://recorder_progress",
                            json!({
                                "nodeId": node_id,
                                "frames": 0u64,
                                "sampleRate": sample_rate,
                                "stopped": true,
                                "error": e.to_string(),
                            }),
                        );
                        return;
                    }
                };

            // A crash loses at most one flush interval of audio.
            const FLUSH_INTERVAL: Duration = Duration::from_secs(2);
            const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
            let mut last_flush = std::time::Instant::now();
            let mut last_progress = std::time::Instant::now();
            let mut frames_written: u64 = 0;
            let mut encoder = encoder;

            worker.run(stop_thread, pacing, |block| {
                encoder.write_stereo(block)?;
                frames_written += (block.len() / 2) as u64;

                if last_flush.elapsed() >= FLUSH_INTERVAL {
                    if let Err(e) = encoder.flush() {
                        warn!(error = %e, "recorder flush failed");
                    }
                    last_flush = std::time::Instant::now();
                }
                if last_progress.elapsed() >= PROGRESS_INTERVAL {
                    let _ = app.emit(
                        "audio://recorder_progress",
                        json!({
                            "nodeId": node_id,
                            "frames": frames_written,
                            "sampleRate": sample_rate,
                        }),
                    );
                    last_progress = std::time::Instant::now();
                }
                Ok(())
            });

            let _ = app.emit(
                "audio://recorder_progress",
                json!({
                    "nodeId": node_id,
                    "frames": frames_written,
                    "sampleRate": sample_rate,
                    "stopped": true,
                }),
            );

            if let Err(e) = encoder.finalize() {
                warn!(error = %e, "recorder finalize failed");
            }
        })
        .map_err(|e| AppError::Stream(format!("spawn recorder thread: {e}")))?;

    Ok((
        RecorderWorker {
            stop,
            join: Some(join),
        },
        ctrl,
    ))
}
