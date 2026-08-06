use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use audio_thread_priority::{
    demote_current_thread_from_real_time, promote_current_thread_to_real_time, RtPriorityHandle,
};
use rtrb::{Producer, RingBuffer};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::audio::clock::{ClockSource, DeviceFillClock, SystemClockTicker};
use crate::audio::effects::{update_meter, MeterHandle};
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

pub(super) use platform::{resolve_speaker, start_speaker_stream, SpeakerHandle, SpeakerResolved};

// No live inputs -> fall back to 48 kHz for the recorder.
const RECORDER_DEFAULT_SR: u32 = 48_000;

// Ring length in frames; multiplied by the device channel count at open so the
// buffered span stays ~340 ms @ 48 kHz at any channel count, absorbing cpal /
// scheduler jitter and output-clock drift.
pub(super) const SPEAKER_RING_CAPACITY_FRAMES: usize = 16_384;

// Ring headroom the fill clock paces the worker towards: enough to absorb a
// DSP-side spike without the device clock -- not the wall clock -- ever
// seeing an empty ring. 3 blocks = 64 ms @ 48 kHz / DSP_BLOCK_FRAMES.
pub(super) const SPEAKER_TARGET_FILL_BLOCKS: usize = 3;

pub(super) enum ResolvedOutput {
    Speaker(SpeakerResolved),
    File {
        path: PathBuf,
        sample_rate: u32,
        format: RecordingFormat,
        channels: u16,
    },
    // The DAG produces at 48 kHz; the send rings are wired inside
    // `build_output_graph`, so nothing device-specific to resolve here. Covers
    // both direct-IP and WebRTC senders.
    WireSender,
}

impl ResolvedOutput {
    pub(super) fn sample_rate(&self) -> u32 {
        match self {
            ResolvedOutput::Speaker(s) => s.sample_rate,
            ResolvedOutput::File { sample_rate, .. } => *sample_rate,
            ResolvedOutput::WireSender => crate::audio::netaudio::SR,
        }
    }
}

pub(super) fn resolve_output(
    out: &ValidOutput,
    file_sr_hint: Option<u32>,
) -> AppResult<ResolvedOutput> {
    match &out.spec {
        OutputSpec::Speaker {
            device_id,
            sample_rate,
        } => Ok(ResolvedOutput::Speaker(platform::resolve_speaker(
            device_id,
            *sample_rate,
        )?)),
        OutputSpec::FileRecording {
            file_path,
            format,
            channels,
        } => Ok(ResolvedOutput::File {
            path: PathBuf::from(file_path),
            sample_rate: file_sr_hint.unwrap_or(RECORDER_DEFAULT_SR),
            format: *format,
            channels: *channels,
        }),
        OutputSpec::NetSender { .. } | OutputSpec::WebRtcSend { .. } => {
            Ok(ResolvedOutput::WireSender)
        }
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

// Per-callback counters for diagnosing the device's actual pull rate against
// the DSP worker's supply rate -- distinguishes "device asking for more than
// real time implies" from "ring nobody fills". Read once a second by the
// non-RT tick thread (`meter::spawn_xrun_thread`); every write here is
// `Ordering::Relaxed`, no allocation, no other sync.
#[derive(Clone)]
pub(super) struct SpeakerIo {
    /// Samples cpal's `fill` was asked for (`out.len()`), summed across callbacks.
    pub requested: Arc<AtomicU64>,
    /// Samples actually popped off the ring (`bulk_pop`'s return), summed across callbacks.
    pub read: Arc<AtomicU64>,
    /// Number of `fill` invocations.
    pub callbacks: Arc<AtomicU64>,
}

impl SpeakerIo {
    fn new() -> Self {
        Self {
            requested: Arc::new(AtomicU64::new(0)),
            read: Arc::new(AtomicU64::new(0)),
            callbacks: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Speaker streams still able to call back into us. Counts handles rather than
/// `fill` closures: cpal's coreaudio backend leaks the closure itself (see
/// `SpeakerHandle`'s Drop), so a closure-based count would never come back
/// down even once the stream is stopped. Exceeding the number of speaker
/// outputs means a stream outlived its worker.
pub(super) static LIVE_SPEAKER_STREAMS: AtomicI64 = AtomicI64::new(0);

/// Held by `SpeakerHandle` so the count follows the stream's real lifetime.
pub(super) struct StreamGuard;

impl StreamGuard {
    pub(super) fn new() -> Self {
        LIVE_SPEAKER_STREAMS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        LIVE_SPEAKER_STREAMS.fetch_sub(1, Ordering::Relaxed);
    }
}

// Builds the speaker ring plus the cpal-side `fill` closure and a fill-level
// handle shared with the worker's sink -- one ring shape for all platforms.
pub(super) fn speaker_ring(
    out_channels: usize,
) -> (
    Producer<f32>,
    impl FnMut(&mut [f32], usize) + Send + 'static,
    Arc<AtomicI64>,
    SpeakerIo,
) {
    let (producer, mut consumer) =
        RingBuffer::<f32>::new(SPEAKER_RING_CAPACITY_FRAMES * out_channels);
    let level = Arc::new(AtomicI64::new(0));
    let level_cb = level.clone();
    let io = SpeakerIo::new();
    let io_cb = io.clone();
    let fill = move |out: &mut [f32], _frames: usize| {
        let read = streams::bulk_pop(&mut consumer, out);
        level_cb.fetch_sub((read / out_channels) as i64, Ordering::Relaxed);
        io_cb
            .requested
            .fetch_add(out.len() as u64, Ordering::Relaxed);
        io_cb.read.fetch_add(read as u64, Ordering::Relaxed);
        io_cb.callbacks.fetch_add(1, Ordering::Relaxed);
    };
    (producer, fill, level, io)
}

// Held for the worker's lifetime: dropping the handle restores normal scheduling.
struct RtThread(Option<RtPriorityHandle>);

impl RtThread {
    fn promote(worker: &'static str, sample_rate: u32) -> Self {
        match promote_current_thread_to_real_time(DSP_BLOCK_FRAMES as u32, sample_rate) {
            Ok(handle) => {
                info!(worker, "worker thread promoted to real-time");
                Self(Some(handle))
            }
            Err(e) => {
                warn!(worker, error = %e, "real-time promotion failed, running at normal priority");
                Self(None)
            }
        }
    }
}

impl Drop for RtThread {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            if let Err(e) = demote_current_thread_from_real_time(handle) {
                warn!(error = %e, "real-time demotion failed");
            }
        }
    }
}

// Shared by both platforms' `start_speaker_stream`: a device-fill-paced
// worker that mixes the output sub-graph and bulk-pushes blocks into the
// speaker ring.
pub(super) fn spawn_speaker_worker(
    mut producer: Producer<f32>,
    level: Arc<AtomicI64>,
    sample_rate: u32,
    channels: usize,
    graph: OutputGraph,
    meter: MeterHandle,
) -> AppResult<(SpeakerWorker, WorkerCtrl)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (worker, ctrl) = dsp_worker(graph);
    let target_frames = SPEAKER_TARGET_FILL_BLOCKS * DSP_BLOCK_FRAMES;
    let clock: Box<dyn ClockSource> = Box::new(DeviceFillClock::new(
        sample_rate,
        DSP_BLOCK_FRAMES,
        target_frames,
        level.clone(),
    ));
    let pacing = WorkerPacing::Clock(clock);
    let join = thread::Builder::new()
        .name(format!("speaker:{sample_rate}"))
        .spawn(move || {
            let _rt = RtThread::promote("speaker", sample_rate);
            worker.run(stop_thread, pacing, |block| {
                update_meter(&meter, block, channels);
                let written = streams::bulk_push_counted(
                    &mut producer,
                    block,
                    &crate::audio::health::SPEAKER_RING_OVERRUN_SAMPLES,
                );
                level.fetch_add((written / channels) as i64, Ordering::Relaxed);
                Ok(())
            });
        })
        .map_err(|e| AppError::Stream(format!("spawn speaker worker: {e}")))?;
    Ok((
        SpeakerWorker {
            stop,
            join: Some(join),
        },
        ctrl,
    ))
}

// Drives analyzers when there's no real output; sink discards the mix.
pub(super) fn start_monitor_worker(graph: OutputGraph) -> AppResult<(RecorderWorker, WorkerCtrl)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    // Live monitors are paced by wall-clock like a speaker, not by source
    // availability (that's for file rendering, which may outrun real time). This
    // keeps meters/scopes at real time and, crucially, consumes network-sourced
    // audio (WebRTC) at the rate it arrives instead of draining its jitter buffer.
    let sample_rate = graph.sample_rate();
    let ticker = SystemClockTicker::new(sample_rate, DSP_BLOCK_FRAMES);
    let (worker, ctrl) = dsp_worker(graph);
    let pacing = WorkerPacing::Clock(Box::new(ticker));
    let join = thread::Builder::new()
        .name("monitor".into())
        .spawn(move || {
            let _rt = RtThread::promote("monitor", sample_rate);
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

// Clock-paced worker for a wire-sender output (direct-IP or WebRTC). The Consumer node pushes each
// channel into its send ring inside `process_block`; the sink is a no-op since
// the background UDP task does the transmitting. Catch-up pacing: a scheduler
// hiccup must not lose wire time -- the send rings are elastic, and lost time
// otherwise builds capture backlog until the trim splices it (a click baked
// into the stream).
pub(super) fn start_wire_sender_worker(
    graph: OutputGraph,
) -> AppResult<(RecorderWorker, WorkerCtrl)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let sample_rate = graph.sample_rate();
    let ticker = SystemClockTicker::with_catchup(sample_rate, DSP_BLOCK_FRAMES, 8);
    let (worker, ctrl) = dsp_worker(graph);
    let pacing = WorkerPacing::Clock(Box::new(ticker));
    let join = thread::Builder::new()
        .name("netsender".into())
        .spawn(move || {
            let _rt = RtThread::promote("netsender", sample_rate);
            worker.run(stop_thread, pacing, |_block| Ok(()));
        })
        .map_err(|e| AppError::Stream(format!("spawn net sender worker: {e}")))?;
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
    channels: u16,
    graph: OutputGraph,
    app: AppHandle,
) -> AppResult<(RecorderWorker, WorkerCtrl)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (worker, ctrl) = dsp_worker(graph);
    let pacing = WorkerPacing::OnAvailability;

    // No real-time promotion: this worker is availability-paced and blocks on encoder file I/O.
    let join = thread::Builder::new()
        .name(format!("recorder:{}", path.display()))
        .spawn(move || {
            // Inside the worker thread so slow encoder init (libopus,
            // libmp3lame, AVAudioFile) doesn't stagger recorder starts.
            let encoder: Box<dyn AudioEncoder> =
                match build_encoder(&path, sample_rate, channels, format) {
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
                encoder.write_interleaved(block)?;
                frames_written += (block.len() / channels as usize) as u64;

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
