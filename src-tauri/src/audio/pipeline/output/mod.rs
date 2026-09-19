use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rtrb::{Producer, RingBuffer};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::warn;

use crate::audio::clock::{ClockSource, DeviceFillClock, SystemClockTicker};
use crate::audio::effects::{update_meter, MeterHandle, WaveformHandle};
use crate::audio::encoders::{build_encoder, validate_append_target, AudioEncoder};
use crate::audio::graph::{NetCodec, OutputSpec, RecordingFormat, RecordingMode, ValidOutput};
use crate::audio::resample::FixedRateResampler;
use crate::audio::streams;
use crate::error::{AppError, AppResult};

use super::dag::{ring_capacity_frames, OutputGraph, DSP_BLOCK_FRAMES};
use super::worker::{dsp_worker, WorkerCtrl};

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

// Floor for the adaptive fill target: enough to absorb a DSP-side spike
// without the device clock -- not the wall clock -- ever seeing an empty ring.
// 3 blocks = 64 ms @ 48 kHz / DSP_BLOCK_FRAMES.
pub(super) const SPEAKER_TARGET_FILL_BLOCKS: usize = 3;

// Extra frames held beyond the device's own callback buffer so the ring never
// sits exactly empty when the next callback lands.
const SPEAKER_TARGET_MARGIN_BLOCKS: usize = 2;

#[inline]
fn pipeline_frames_to_device_frames(frames: usize, pipeline_rate: u32, device_rate: u32) -> usize {
    ((frames as u64 * device_rate.max(1) as u64 + pipeline_rate.max(1) as u64 / 2)
        / pipeline_rate.max(1) as u64) as usize
}

pub(super) enum ResolvedOutput {
    Speaker(SpeakerResolved),
    File {
        path: PathBuf,
        sample_rate: u32,
        format: RecordingFormat,
        channels: u16,
        append: bool,
        /// Existing per-channel frame count when appending, so counters start
        /// from the file's current length instead of zero.
        base_frames: u64,
    },
    // The DAG produces at its configured rate; the send rings are wired inside
    // `build_output_graph`, so nothing device-specific to resolve here. Covers
    // both direct-IP and WebRTC senders.
    WireSender(u32),
}

impl ResolvedOutput {
    pub(super) fn sample_rate(&self) -> u32 {
        match self {
            ResolvedOutput::Speaker(s) => s.sample_rate,
            ResolvedOutput::File { sample_rate, .. } => *sample_rate,
            ResolvedOutput::WireSender(sr) => *sr,
        }
    }
}

pub(super) fn resolve_output(
    out: &ValidOutput,
    file_sr_hint: Option<u32>,
) -> AppResult<ResolvedOutput> {
    match &out.spec {
        OutputSpec::Speaker { device_id } => Ok(ResolvedOutput::Speaker(
            platform::resolve_speaker(device_id)?,
        )),
        OutputSpec::FileRecording {
            file_path,
            format,
            channels,
            mode,
            sample_rate: pinned,
        } => {
            let path = PathBuf::from(file_path);
            let sample_rate = pinned.or(file_sr_hint).unwrap_or(RECORDER_DEFAULT_SR);
            let append = *mode == RecordingMode::Append;
            // Overwrite erases the file up front -- the confirmed modal's
            // contract -- so every encoder starts from a clean path: the FLAC
            // writer refuses existing files, and CoreAudio's AAC rejects
            // arbitrary sample rates, custom ones included.
            if *mode == RecordingMode::Overwrite && path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| AppError::Stream(format!("remove {}: {e}", path.display())))?;
            }
            if let RecordingFormat::Aac { bitrate } = format {
                // Probed limits of Apple's AAC encoder (macOS 14): it encodes
                // only 32/44.1/48 kHz, with bitrate bounds scaling by channel
                // count under a 320 kbps absolute cap.
                let channels = u32::from(*channels);
                let (min_per_ch, max_per_ch) = match sample_rate {
                    32_000 => (24_000, 96_000),
                    44_100 | 48_000 => (32_000, 256_000),
                    _ => {
                        return Err(AppError::Validation(format!(
                            "AAC supports only 32000, 44100 and 48000 Hz, {sample_rate} Hz requested"
                        )));
                    }
                };
                let bounds = (min_per_ch * channels, (max_per_ch * channels).min(320_000));
                if *bitrate < bounds.0 || *bitrate > bounds.1 {
                    return Err(AppError::Validation(format!(
                        "AAC bitrate {bitrate} bps is out of {}..{} at {sample_rate} Hz",
                        bounds.0, bounds.1
                    )));
                }
            }
            if let RecordingFormat::Mp3 { bitrate_kbps } = format {
                // LAME CBR ranges track the MPEG layer of the sample rate.
                let bounds = match sample_rate {
                    32_000 | 44_100 | 48_000 => (32, 320),
                    16_000 | 22_050 | 24_000 => (8, 160),
                    _ => (8, 64),
                };
                if *bitrate_kbps < bounds.0 || *bitrate_kbps > bounds.1 {
                    return Err(AppError::Validation(format!(
                        "MP3 bitrate {bitrate_kbps} kbps is out of {}..{} at {sample_rate} Hz",
                        bounds.0, bounds.1
                    )));
                }
            }
            let base_frames = if append && path.exists() {
                validate_append_target(&path, sample_rate, *channels, *format)?
            } else {
                0
            };
            Ok(ResolvedOutput::File {
                path,
                sample_rate,
                format: *format,
                channels: *channels,
                append,
                base_frames,
            })
        }
        OutputSpec::NetSender {
            codec, sample_rate, ..
        } => {
            let sr = if *codec == NetCodec::Opus {
                crate::audio::netaudio::SR
            } else {
                sample_rate
                    .or(file_sr_hint)
                    .unwrap_or(crate::audio::netaudio::SR)
            };
            Ok(ResolvedOutput::WireSender(sr))
        }
        OutputSpec::WebRtcSend { .. } => Ok(ResolvedOutput::WireSender(
            file_sr_hint.unwrap_or(crate::audio::netaudio::SR),
        )),
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
    /// Native clock rate of the physical output stream. This differs from the
    /// graph's pipeline rate when the output resampler is active.
    pub sample_rate: Arc<AtomicU32>,
    /// Samples cpal's `fill` was asked for (`out.len()`), summed across callbacks.
    pub requested: Arc<AtomicU64>,
    /// Samples actually popped off the ring (`bulk_pop`'s return), summed across callbacks.
    pub read: Arc<AtomicU64>,
    /// Number of `fill` invocations.
    pub callbacks: Arc<AtomicU64>,
    /// Adaptive fill target in frames, sized to the device's own buffer by the
    /// callback. The worker's clock steers to it; the UI reads it back as the
    /// current output latency.
    pub target_frames: Arc<AtomicI64>,
    /// Lookahead delay compensation has aligned the graph to (frames) -- the
    /// deepest cumulative effect latency on the path to this output.
    pub graph_latency_frames: usize,
}

impl SpeakerIo {
    fn new(sample_rate: u32, target_frames: Arc<AtomicI64>, graph_latency_frames: usize) -> Self {
        Self {
            sample_rate: Arc::new(AtomicU32::new(sample_rate)),
            requested: Arc::new(AtomicU64::new(0)),
            read: Arc::new(AtomicU64::new(0)),
            callbacks: Arc::new(AtomicU64::new(0)),
            target_frames,
            graph_latency_frames,
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

// Builds the speaker ring plus the cpal-side `fill` closure, a fill-level
// handle shared with the worker's sink, and the adaptive fill target the
// worker's clock steers toward -- one ring shape for all platforms.
pub(super) fn speaker_ring(
    out_channels: usize,
    pipeline_rate: u32,
    device_rate: u32,
    graph_latency_frames: usize,
) -> (
    Producer<f32>,
    impl FnMut(&mut [f32], usize) + Send + 'static,
    Arc<AtomicI64>,
    Arc<AtomicI64>,
    SpeakerIo,
) {
    // One second at the actual device rate, so high-rate and wide-channel
    // streams have the same time capacity as 48 kHz stereo.
    let capacity_frames = ring_capacity_frames(device_rate);
    let (producer, mut consumer) = RingBuffer::<f32>::new(capacity_frames * out_channels);
    let level = Arc::new(AtomicI64::new(0));
    let level_cb = level.clone();
    let target = Arc::new(AtomicI64::new(pipeline_frames_to_device_frames(
        SPEAKER_TARGET_FILL_BLOCKS * DSP_BLOCK_FRAMES,
        pipeline_rate,
        device_rate,
    ) as i64));
    let target_cb = target.clone();
    let io = SpeakerIo::new(device_rate, target.clone(), graph_latency_frames);
    let io_cb = io.clone();
    let fill = move |out: &mut [f32], callback_frames: usize| {
        let read = streams::bulk_pop(&mut consumer, out);
        level_cb.fetch_sub((read / out_channels) as i64, Ordering::Relaxed);
        // Size the fill target to the device's own callback buffer so the ring
        // always bridges one full callback. A healthy device asks for a few
        // blocks and the floor holds; a large-buffer device (PipeWire handing
        // out ~250 ms buffers) grows the target and runs at that latency instead
        // of underrunning at a fraction of real time.
        let min = pipeline_frames_to_device_frames(
            SPEAKER_TARGET_FILL_BLOCKS * DSP_BLOCK_FRAMES,
            pipeline_rate,
            device_rate,
        );
        let margin = pipeline_frames_to_device_frames(
            SPEAKER_TARGET_MARGIN_BLOCKS * DSP_BLOCK_FRAMES,
            pipeline_rate,
            device_rate,
        );
        let dev_frames = if callback_frames == 0 {
            out.len() / out_channels
        } else {
            callback_frames
        };
        let max = capacity_frames.saturating_sub(dev_frames + margin).max(min);
        target_cb.store(
            (dev_frames + margin).clamp(min, max) as i64,
            Ordering::Relaxed,
        );
        io_cb
            .requested
            .fetch_add(out.len() as u64, Ordering::Relaxed);
        io_cb.read.fetch_add(read as u64, Ordering::Relaxed);
        io_cb.callbacks.fetch_add(1, Ordering::Relaxed);
    };
    (producer, fill, level, target, io)
}

fn output_resampler(
    pipeline_rate: u32,
    device_rate: u32,
    channels: usize,
) -> AppResult<Option<FixedRateResampler>> {
    if pipeline_rate == device_rate {
        Ok(None)
    } else {
        FixedRateResampler::new(pipeline_rate, device_rate, DSP_BLOCK_FRAMES, channels).map(Some)
    }
}

// Shared by both platforms' `start_speaker_stream`: a device-fill-paced
// worker that mixes the output sub-graph and bulk-pushes blocks into the
// speaker ring.
pub(super) fn spawn_speaker_worker(
    mut producer: Producer<f32>,
    level: Arc<AtomicI64>,
    target: Arc<AtomicI64>,
    device_sample_rate: Arc<AtomicU32>,
    channels: usize,
    graph: OutputGraph,
    meter: MeterHandle,
) -> AppResult<(SpeakerWorker, WorkerCtrl)> {
    let pipeline_rate = graph.sample_rate();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (worker, ctrl) = dsp_worker(graph);
    let clock: Box<dyn ClockSource> = Box::new(DeviceFillClock::new(
        pipeline_rate,
        device_sample_rate.clone(),
        DSP_BLOCK_FRAMES,
        level.clone(),
        target,
    ));
    let initial_device_rate = device_sample_rate.load(Ordering::Relaxed);
    let mut resampler = output_resampler(pipeline_rate, initial_device_rate, channels)?;
    let mut resampled = vec![
        0.0_f32;
        resampler
            .as_ref()
            .map(|r| r.out_max() * channels)
            .unwrap_or(DSP_BLOCK_FRAMES * channels)
    ];
    let mut resampled_channels = 0;
    let join = thread::Builder::new()
        .name(format!("speaker:{initial_device_rate}"))
        .spawn(move || {
            worker.run(
                stop_thread,
                clock,
                Some(("speaker", pipeline_rate)),
                |block, active_channels| {
                    update_meter(&meter, block, channels);
                    let device_block = if let Some(resampler) = &mut resampler {
                        resampled_channels = resampled_channels.max(active_channels);
                        let written = resampler.process_chunk_into(
                            block,
                            resampled_channels,
                            &mut resampled,
                        )?;
                        &resampled[..written]
                    } else {
                        block
                    };
                    let written = streams::bulk_push_counted(
                        &mut producer,
                        device_block,
                        &crate::audio::health::SPEAKER_RING_OVERRUN_SAMPLES,
                    );
                    level.fetch_add((written / channels) as i64, Ordering::Relaxed);
                    Ok(())
                },
            );
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
    let join = thread::Builder::new()
        .name("monitor".into())
        .spawn(move || {
            worker.run(
                stop_thread,
                Box::new(ticker),
                Some(("monitor", sample_rate)),
                |_block, _| Ok(()),
            );
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
    let join = thread::Builder::new()
        .name("netsender".into())
        .spawn(move || {
            worker.run(
                stop_thread,
                Box::new(ticker),
                Some(("netsender", sample_rate)),
                |_block, _| Ok(()),
            );
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
    append: bool,
    base_frames: u64,
    graph: OutputGraph,
    app: AppHandle,
) -> AppResult<(RecorderWorker, WorkerCtrl, WaveformHandle)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (worker, ctrl) = dsp_worker(graph);
    // Transport-paced: recording must follow the wall clock, not the source.
    // A file source decodes faster than real time; availability pacing would
    // drain it as fast as it arrives and over-run (a 1 s clip becomes 1:22).
    let clock: Box<dyn ClockSource> =
        Box::new(SystemClockTicker::new(sample_rate, DSP_BLOCK_FRAMES));

    // Scope-style waveform feed, emitted to the UI by the meter tick thread.
    let wave = WaveformHandle::for_recorder(node_id.clone(), sample_rate, base_frames);
    let wave_thread = wave.clone();
    let session = wave.session;

    // No real-time promotion: this worker blocks on encoder file I/O.
    let channels_usize = channels as usize;
    let join = thread::Builder::new()
        .name(format!("recorder:{}", path.display()))
        .spawn(move || {
            // Inside the worker thread so slow encoder init (libopus,
            // libmp3lame, AVAudioFile) doesn't stagger recorder starts.
            let encoder: Box<dyn AudioEncoder> =
                match build_encoder(&path, sample_rate, channels, format, append) {
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
                                "session": session,
                                "baseFrames": base_frames,
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
            // Append starts from the file's existing length, so the readouts
            // reflect total content, not just this session's bytes.
            let mut frames_written: u64 = base_frames;
            let mut encoder = encoder;

            worker.run(stop_thread, clock, None, |block, _| {
                encoder.write_interleaved(block)?;
                frames_written += (block.len() / channels_usize) as u64;
                wave_thread.push_interleaved(block, block.len() / channels_usize, base_frames);

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
                            "session": session,
                            "baseFrames": base_frames,
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
                    "session": session,
                    "baseFrames": base_frames,
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
        wave,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::pipeline::dag::RESAMPLE_CHUNK;
    use std::collections::HashMap;

    #[test]
    fn test_output_resampler_bypassed_when_rates_match() {
        // When initial_device_rate == pipeline_rate (e.g. 96 kHz pipeline and 96 kHz speaker),
        // no output resampler must be allocated, preserving 1:1 bit-transparent playback.
        for rate in [44_100, 48_000, 96_000, 192_000] {
            let resampler = output_resampler(rate, rate, 2).unwrap();
            assert!(
                resampler.is_none(),
                "output resampler should be None for matching rate {rate}"
            );
        }
    }

    #[test]
    fn test_output_resampler_only_allocated_when_rates_differ() {
        let resampler = output_resampler(96_000, 48_000, 2).unwrap();
        assert!(
            resampler.is_some(),
            "output resampler must be Some when rates differ"
        );
    }

    #[test]
    fn test_output_bit_transparent_sample_passthrough() {
        // Verify that when output resampler is None, samples pass directly to the device buffer.
        let total_samples = RESAMPLE_CHUNK * 2;
        let mut block = vec![0.0f32; total_samples];
        for (i, sample) in block.iter_mut().enumerate() {
            *sample = ((i as f32) * 0.005).cos();
        }

        let mut resampler = output_resampler(96_000, 96_000, 2).unwrap();
        let mut resampled = Vec::new();
        let active_channels = 2;
        let mut resampled_channels = 0;

        let device_block = if let Some(resampler) = &mut resampler {
            resampled_channels = resampled_channels.max(active_channels);
            let written = resampler
                .process_chunk_into(&block[..total_samples], resampled_channels, &mut resampled)
                .unwrap();
            &resampled[..written]
        } else {
            &block[..total_samples]
        };

        assert_eq!(device_block.len(), total_samples);
        for (a, b) in device_block.iter().zip(block.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn pipeline_frames_convert_to_device_frames_with_rounding() {
        assert_eq!(pipeline_frames_to_device_frames(1024, 48_000, 48_000), 1024);
        assert_eq!(pipeline_frames_to_device_frames(1024, 48_000, 96_000), 2048);
        assert_eq!(pipeline_frames_to_device_frames(1024, 96_000, 48_000), 512);
        // Half-frame rounds up (512*48/96 = 256.0 → 256).
        assert_eq!(pipeline_frames_to_device_frames(512, 96_000, 48_000), 256);
        // Zero pipeline rate falls back to 1: frames * device_rate / 1.
        assert_eq!(
            pipeline_frames_to_device_frames(1024, 0, 48_000),
            1024 * 48_000
        );
    }

    #[test]
    fn resolved_output_reports_rates() {
        assert_eq!(ResolvedOutput::WireSender(44_100).sample_rate(), 44_100);
        assert_eq!(
            ResolvedOutput::File {
                path: PathBuf::from("/tmp/x.wav"),
                sample_rate: 96_000,
                format: RecordingFormat::Wav {
                    bit_depth: crate::audio::graph::WavBitDepth::F32
                },
                channels: 2,
                append: false,
                base_frames: 0,
            }
            .sample_rate(),
            96_000
        );
    }

    #[test]
    fn speaker_ring_reads_what_was_pushed() {
        let (mut prod, mut fill, level, target, io) = speaker_ring(2, 48_000, 48_000, 0);
        let data: Vec<f32> = (0..1024 * 2).map(|i| i as f32 * 0.001).collect();
        // Ring is one second at the device rate — far larger than a block.
        assert_eq!(
            prod.push_entire_slice(&data).ok(),
            Some(()),
            "ring must absorb a block"
        );
        let mut out = vec![0.0f32; 1024 * 2];
        fill(&mut out, 0);
        assert_eq!(out, data);
        // The fill decrements the gauge; the push-side increment lives in the
        // worker, so the standalone gauge reads -read/out_channels.
        assert_eq!(level.load(Ordering::Relaxed), -1024);
        assert!(target.load(Ordering::Relaxed) > 0);
        assert!(io.requested.load(Ordering::Relaxed) > 0);
        assert!(io.read.load(Ordering::Relaxed) > 0);
        assert_eq!(io.callbacks.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn stream_guard_counts_live_streams() {
        let before = LIVE_SPEAKER_STREAMS.load(Ordering::Relaxed);
        {
            let _guard = StreamGuard::new();
            assert_eq!(LIVE_SPEAKER_STREAMS.load(Ordering::Relaxed), before + 1);
        }
        assert_eq!(LIVE_SPEAKER_STREAMS.load(Ordering::Relaxed), before);
    }

    #[test]
    fn speaker_worker_pushes_blocks_into_the_ring() {
        use super::super::dag::build_output_graph;
        use crate::audio::effects::EffectRegistry;
        use crate::audio::graph::{EdgeSpec, GraphSpec, InputSpec, NodeKind, NodeSpec, ValidGraph};

        // Build a monitor-style graph (mic source ring, no real device).
        let g = GraphSpec {
            sample_rate: None,
            nodes: vec![
                NodeSpec {
                    id: "m".into(),
                    kind: NodeKind::Microphone,
                    data: serde_json::json!({ "deviceId": "dev" }),
                },
                NodeSpec {
                    id: "s".into(),
                    kind: NodeKind::Speaker,
                    data: serde_json::json!({ "deviceId": "dev" }),
                },
            ],
            edges: vec![EdgeSpec {
                id: "e".into(),
                source: "m".into(),
                source_handle: None,
                target: "s".into(),
                target_handle: None,
            }],
        };
        let valid: ValidGraph = g.validate().expect("valid");
        let mut producer_pairs = Vec::new();
        let native = valid
            .inputs
            .iter()
            .map(|i| (i.id.clone(), 48_000))
            .collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let mut reg = EffectRegistry::new();
        let built = build_output_graph(
            Some("s"),
            48_000,
            false,
            &valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            HashMap::new(),
        )
        .expect("build");

        let (prod, _fill, level, target, io) = speaker_ring(2, 48_000, 48_000, 0);
        let device_sr = Arc::new(AtomicU32::new(48_000));
        let meter = MeterHandle::new("w".into());
        let (_worker, _ctrl) = spawn_speaker_worker(
            prod,
            level.clone(),
            target.clone(),
            device_sr.clone(),
            2,
            built.graph,
            meter,
        )
        .expect("spawn worker");
        // The worker must pump blocks within a few hundred ms.
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            level.load(Ordering::Relaxed) > 0,
            "worker pushed audio into the ring"
        );
        assert_eq!(
            io.callbacks.load(Ordering::Relaxed),
            0,
            "no device callback is attached"
        );
    }

    #[test]
    fn monitor_worker_runs_at_wall_clock() {
        use super::super::dag::build_output_graph;
        use crate::audio::effects::EffectRegistry;
        use crate::audio::graph::{EdgeSpec, GraphSpec, NodeKind, NodeSpec, ValidGraph};
        use std::collections::HashMap;

        let g = GraphSpec {
            sample_rate: None,
            nodes: vec![
                NodeSpec {
                    id: "m".into(),
                    kind: NodeKind::Microphone,
                    data: serde_json::json!({ "deviceId": "dev" }),
                },
                NodeSpec {
                    id: "lm".into(),
                    kind: NodeKind::LevelMeter,
                    data: serde_json::json!({}),
                },
            ],
            edges: vec![EdgeSpec {
                id: "e".into(),
                source: "m".into(),
                source_handle: None,
                target: "lm".into(),
                target_handle: None,
            }],
        };
        let valid: ValidGraph = g.validate().expect("valid");
        let mut producer_pairs = Vec::new();
        let native = valid
            .inputs
            .iter()
            .map(|i| (i.id.clone(), 48_000))
            .collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let mut reg = EffectRegistry::new();
        let built = build_output_graph(
            None,
            48_000,
            true,
            &valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            HashMap::new(),
        )
        .expect("build");

        let (recorder, _ctrl) = start_monitor_worker(built.graph).expect("spawn monitor");
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            built.output.blocks.load(Ordering::Relaxed) > 0,
            "monitor produced blocks in real time"
        );
        drop(recorder);
        let after_stop = built.output.blocks.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            built.output.blocks.load(Ordering::Relaxed),
            after_stop,
            "worker stopped on drop"
        );
    }
}
