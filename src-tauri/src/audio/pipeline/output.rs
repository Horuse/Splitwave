use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rtrb::{Producer, RingBuffer};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::audio::clock::{ClockSource, SystemClockTicker};
#[cfg(target_os = "macos")]
use crate::audio::device::{self, DeviceKind};
use crate::audio::encoders::{build_encoder, AudioEncoder};
use crate::audio::graph::{OutputSpec, RecordingFormat, ValidOutput};
use crate::audio::streams;
use crate::error::{AppError, AppResult};

use super::dag::{OutputGraph, DSP_BLOCK_FRAMES};
use super::worker::{dsp_worker, WorkerCtrl, WorkerPacing};
#[cfg(target_os = "macos")]
use super::native_config;

// No live inputs -> fall back to 48 kHz for the recorder.
const RECORDER_DEFAULT_SR: u32 = 48_000;

// 32k f32 samples = ~340 ms @ 48 kHz stereo; absorbs cpal/scheduler jitter.
const SPEAKER_RING_CAPACITY: usize = 32_768;

// Bluetooth AUHAL often returns DeviceNotAvailable on first bind; retry covers settling.
#[cfg(target_os = "macos")]
const SPEAKER_MAX_ATTEMPTS: u32 = 3;
#[cfg(target_os = "macos")]
const SPEAKER_RETRY_DELAY: Duration = Duration::from_millis(300);

#[cfg(target_os = "macos")]
pub(super) struct SpeakerResolved {
    pub device: cpal::Device,
    pub config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
    pub out_channels: usize,
    pub sample_rate: u32,
}

#[cfg(not(target_os = "macos"))]
pub(super) struct SpeakerResolved {
    pub node_id: String,
    pub sample_rate: u32,
}

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
        #[cfg(target_os = "macos")]
        OutputSpec::Speaker { device_id } => {
            let device = device::find(DeviceKind::Output, device_id)?;
            let native = native_config(DeviceKind::Output, &device, device_id)?;
            Ok(ResolvedOutput::Speaker(SpeakerResolved {
                device,
                config: native.config,
                sample_format: native.sample_format,
                out_channels: native.channels as usize,
                sample_rate: native.sample_rate,
            }))
        }
        #[cfg(not(target_os = "macos"))]
        OutputSpec::Speaker { device_id } => Ok(ResolvedOutput::Speaker(SpeakerResolved {
            node_id: device_id.clone(),
            sample_rate: 48_000,
        })),
        OutputSpec::FileRecording { file_path, format } => Ok(ResolvedOutput::File {
            path: PathBuf::from(file_path),
            sample_rate: file_sr_hint.unwrap_or(RECORDER_DEFAULT_SR),
            format: *format,
        }),
    }
}

// Substring match on cpal's stable Display -- AppError flattens the variant.
#[cfg(target_os = "macos")]
fn is_device_not_available(e: &AppError) -> bool {
    matches!(e, AppError::Stream(s) if s.contains("no longer available"))
}

// Field order: the stream drops before the worker so the audio callback stops
// before the ring is freed.
pub(super) struct SpeakerHandle {
    #[cfg(target_os = "macos")]
    _stream: cpal::Stream,
    #[cfg(not(target_os = "macos"))]
    _playback: crate::audio::playback::Playback,
    _worker: SpeakerWorker,
}

struct SpeakerWorker {
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

#[cfg(target_os = "macos")]
pub(super) fn start_speaker_stream(
    node_id: &str,
    spec: SpeakerResolved,
    graph: OutputGraph,
    app: &AppHandle,
) -> AppResult<(SpeakerHandle, WorkerCtrl, Arc<AtomicBool>)> {
    use cpal::traits::DeviceTrait;
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

    let mut producer_holder: Option<Producer<f32>> = None;
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

#[cfg(not(target_os = "macos"))]
pub(super) fn start_speaker_stream(
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

fn spawn_speaker_worker(
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
