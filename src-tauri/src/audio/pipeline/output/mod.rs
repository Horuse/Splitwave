use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::json;
use tracing::warn;

use crate::audio::clock::{ClockSource, SystemClockTicker};
use crate::audio::effects::{MeterHandle, WaveformHandle};
use crate::audio::encoders::{build_encoder, validate_append_target, AudioEncoder};
use crate::audio::graph::{NetCodec, OutputSpec, RecordingFormat, RecordingMode, ValidOutput};
use crate::error::{AppError, AppResult};

use super::dag::OutputGraph;
use super::host::Host;
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

mod render;

pub(super) use platform::resolve_speaker;
use platform::{start_speaker_stream, SpeakerHandle, SpeakerResolved};
pub(super) use render::SpeakerIo;
use render::{speaker_link, SpeakerLink, SpeakerRenderer};

use super::virtual_io::{DeviceClock, VirtualSpeaker};

/// A speaker to open: a device, or one that exists only in memory.
pub(super) enum SpeakerTarget {
    Device(SpeakerResolved),
    Virtual {
        id: String,
        speaker: VirtualSpeaker,
        played: Arc<Mutex<Vec<f32>>>,
    },
}

/// An open speaker. Held for its `Drop`: the renderer is taken back with a
/// fade, then the stream stops.
pub(super) enum SpeakerStream {
    Device(#[allow(dead_code)] SpeakerHandle),
    Virtual(#[allow(dead_code)] VirtualSpeakerStream),
}

pub(super) struct VirtualSpeakerStream {
    link: SpeakerLink,
    _clock: DeviceClock,
    _alive: StreamGuard,
}

impl Drop for VirtualSpeakerStream {
    fn drop(&mut self) {
        // Taken back while the clock still calls, so the stop fade plays out;
        // the clock then stops as the fields drop.
        drop(self.link.retire());
    }
}

impl SpeakerTarget {
    pub(super) fn sample_rate(&self) -> u32 {
        match self {
            SpeakerTarget::Device(s) => s.sample_rate,
            SpeakerTarget::Virtual { speaker, .. } => speaker.sample_rate,
        }
    }

    pub(super) fn out_channels(&self) -> usize {
        match self {
            SpeakerTarget::Device(s) => s.out_channels,
            SpeakerTarget::Virtual { speaker, .. } => speaker.channels.max(1) as usize,
        }
    }

    /// Opens the stream and hands it `graph`. Returns the stream, the worker's
    /// control, the flag the stream raises when it dies, and its counters.
    pub(super) fn open(
        self,
        node_id: &str,
        graph: OutputGraph,
        meter: MeterHandle,
        host: &Host,
    ) -> AppResult<(SpeakerStream, WorkerCtrl, Arc<AtomicBool>, SpeakerIo)> {
        match self {
            SpeakerTarget::Device(spec) => {
                let (handle, ctrl, dead, io) =
                    start_speaker_stream(node_id, spec, graph, meter, host)?;
                Ok((SpeakerStream::Device(handle), ctrl, dead, io))
            }
            SpeakerTarget::Virtual {
                id,
                speaker,
                played,
            } => {
                let channels = speaker.channels.max(1) as usize;
                let rate = speaker.sample_rate;
                let period = device_block(graph.block_frames(), graph.sample_rate(), rate) as usize;
                let (renderer, ctrl, io) = speaker_renderer(graph, rate, channels, None, meter)?;
                let (mut link, mut fill) = speaker_callback();
                link.attach(renderer);
                let mut buf = vec![0.0f32; period * channels];
                let clock = DeviceClock::start(&id, rate, 0.0, period, move |frames| {
                    let out = &mut buf[..frames * channels];
                    fill(out, frames);
                    played.lock().unwrap().extend_from_slice(out);
                })?;
                let stream = VirtualSpeakerStream {
                    link,
                    _clock: clock,
                    _alive: StreamGuard::new(),
                };
                Ok((
                    SpeakerStream::Virtual(stream),
                    ctrl,
                    Arc::new(AtomicBool::new(false)),
                    io,
                ))
            }
        }
    }
}

// No live inputs -> fall back to 48 kHz for the recorder.
const RECORDER_DEFAULT_SR: u32 = 48_000;

pub(super) enum ResolvedOutput {
    Speaker(SpeakerTarget),
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
            ResolvedOutput::Speaker(s) => s.sample_rate(),
            ResolvedOutput::File { sample_rate, .. } => *sample_rate,
            ResolvedOutput::WireSender(sr) => *sr,
        }
    }
}

pub(super) fn resolve_output(
    out: &ValidOutput,
    file_sr_hint: Option<u32>,
    host: &Host,
) -> AppResult<ResolvedOutput> {
    match &out.spec {
        OutputSpec::Speaker { device_id } => {
            Ok(ResolvedOutput::Speaker(match host.virtual_devices() {
                Some(devices) => SpeakerTarget::Virtual {
                    id: device_id.clone(),
                    speaker: devices.speaker(device_id)?,
                    played: devices.sink(device_id),
                },
                None => SpeakerTarget::Device(platform::resolve_speaker(device_id)?),
            }))
        }
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
            // Validate the complete recording configuration before honoring
            // the user's confirmed overwrite and touching the existing file.
            if *mode == RecordingMode::Overwrite && path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| AppError::Stream(format!("remove {}: {e}", path.display())))?;
            }
            Ok(ResolvedOutput::File {
                path,
                sample_rate,
                format: *format,
                channels: *channels,
                append,
                base_frames,
            })
        }
        // Opus encodes at 48 kHz, and WebRTC always carries 48 kHz, so their
        // graphs run there and the engine converts the rate block by block;
        // raw PCM goes out at the rate it is set to, else the pipeline's.
        OutputSpec::NetSender {
            codec, sample_rate, ..
        } => {
            let sr = if *codec == NetCodec::Opus {
                Some(crate::audio::netaudio::SR)
            } else {
                sample_rate.or(file_sr_hint)
            };
            sr.map(ResolvedOutput::WireSender)
                .ok_or_else(|| AppError::Validation("network sender has no sample rate".into()))
        }
        OutputSpec::WebRtcSend { .. } => {
            Ok(ResolvedOutput::WireSender(crate::audio::webrtc::OPUS_SR))
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

/// A speaker's device callback plus the handoff that carries its renderer in
/// and back out. Built per open attempt; the renderer is attached only once the
/// stream is up, so a failed open never takes the graph down with its closure.
pub(super) fn speaker_callback() -> (SpeakerLink, impl FnMut(&mut [f32], usize) + Send + 'static) {
    let (link, mut callback) = speaker_link();
    (link, move |out: &mut [f32], frames: usize| {
        callback.fill(out, frames)
    })
}

/// Renderer for a speaker whose stream opened at `device_rate` with
/// `channels` physical channels. `hardware_frames` is what the device reports
/// adding past its buffer, where the OS reports it.
pub(super) fn speaker_renderer(
    mut graph: OutputGraph,
    device_rate: u32,
    channels: usize,
    hardware_frames: Option<u32>,
    meter: MeterHandle,
) -> AppResult<(SpeakerRenderer, WorkerCtrl, SpeakerIo)> {
    graph.set_out_channels(channels);
    SpeakerRenderer::new(graph, device_rate, hardware_frames, meter)
}

/// The engine block expressed in a device's own frames: what its IO buffer
/// should be so one callback carries one block.
pub(super) fn device_block(block_frames: usize, pipeline_rate: u32, device_rate: u32) -> u32 {
    ((block_frames as u64 * device_rate as u64 + pipeline_rate as u64 / 2)
        / pipeline_rate.max(1) as u64)
        .max(1) as u32
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

// Drives analyzers when there's no real output; sink discards the mix.
pub(super) fn start_monitor_worker(graph: OutputGraph) -> AppResult<(RecorderWorker, WorkerCtrl)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    // Live monitors are paced by wall-clock like a speaker, not by source
    // availability (that's for file rendering, which may outrun real time). This
    // keeps meters/scopes at real time and, crucially, consumes network-sourced
    // audio (WebRTC) at the rate it arrives instead of draining its jitter buffer.
    let sample_rate = graph.sample_rate();
    // Ticks one graph block at a time, whatever block the graph was built at.
    let ticker = SystemClockTicker::new(sample_rate, graph.block_frames());
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
    let ticker = SystemClockTicker::with_catchup(sample_rate, graph.block_frames(), 8);
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
    host: Host,
) -> AppResult<(RecorderWorker, WorkerCtrl, WaveformHandle)> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    // Transport-paced: recording must follow the wall clock, not the source.
    // A file source decodes faster than real time; availability pacing would
    // drain it as fast as it arrives and over-run (a 1 s clip becomes 1:22).
    let clock: Box<dyn ClockSource> = Box::new(SystemClockTicker::new(
        graph.sample_rate(),
        graph.block_frames(),
    ));
    let (worker, ctrl) = dsp_worker(graph);

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
                        host.emit(
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
                    host.emit(
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

            host.emit(
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

    /// A host offering no devices: enough for outputs that need none.
    fn no_devices() -> Host {
        Host::with_virtual_devices(Arc::default(), |_, _| {})
    }

    fn recording_output(
        path: &std::path::Path,
        format: RecordingFormat,
        mode: RecordingMode,
    ) -> ValidOutput {
        ValidOutput {
            id: "recording".into(),
            spec: OutputSpec::FileRecording {
                file_path: path.to_string_lossy().into_owned(),
                format,
                channels: 2,
                mode,
                sample_rate: None,
            },
        }
    }

    #[test]
    fn invalid_overwrite_configuration_preserves_existing_file() {
        let path = std::env::temp_dir().join(format!(
            "splitwave-invalid-overwrite-{}.mp3",
            std::process::id()
        ));
        std::fs::write(&path, b"existing recording").expect("fixture");
        let output = recording_output(
            &path,
            RecordingFormat::Mp3 { bitrate_kbps: 999 },
            RecordingMode::Overwrite,
        );

        let error = match resolve_output(&output, Some(48_000), &no_devices()) {
            Ok(_) => panic!("invalid bitrate was accepted"),
            Err(error) => error,
        };
        assert!(format!("{error}").contains("MP3 bitrate"));
        assert_eq!(
            std::fs::read(&path).expect("existing file retained"),
            b"existing recording"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn valid_overwrite_configuration_removes_existing_file() {
        let path = std::env::temp_dir().join(format!(
            "splitwave-valid-overwrite-{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, b"existing recording").expect("fixture");
        let output = recording_output(
            &path,
            RecordingFormat::Wav {
                bit_depth: crate::audio::graph::WavBitDepth::F32,
            },
            RecordingMode::Overwrite,
        );

        resolve_output(&output, Some(48_000), &no_devices()).expect("valid recording output");
        assert!(
            !path.exists(),
            "confirmed overwrite must clear the old path"
        );
    }

    #[test]
    fn wire_senders_run_at_their_wire_rate() {
        use crate::audio::graph::{NetCodec, OpusApplication};
        let net = |codec: NetCodec, sample_rate: Option<u32>| ValidOutput {
            id: "n".into(),
            spec: OutputSpec::NetSender {
                node_id: "n".into(),
                target: "127.0.0.1:5004".parse().unwrap(),
                channels: 2,
                codec,
                opus_bitrate: 96_000,
                opus_application: OpusApplication::Audio,
                sample_rate,
            },
        };
        let rate = |out: &ValidOutput| {
            resolve_output(out, Some(96_000), &no_devices())
                .unwrap()
                .sample_rate()
        };
        assert_eq!(
            rate(&net(NetCodec::Opus, None)),
            48_000,
            "Opus encodes at 48 kHz"
        );
        assert_eq!(
            rate(&net(NetCodec::PcmF32, None)),
            96_000,
            "PCM follows the pipeline"
        );
        assert_eq!(rate(&net(NetCodec::PcmI16, Some(44_100))), 44_100);
        let webrtc = ValidOutput {
            id: "w".into(),
            spec: OutputSpec::WebRtcSend {
                node_id: "w".into(),
                channels: 2,
                opus_bitrate: 96_000,
                opus_application: OpusApplication::Voip,
            },
        };
        assert_eq!(
            rate(&webrtc),
            48_000,
            "WebRTC carries 48 kHz, so its graph runs there"
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
    fn device_block_follows_the_device_rate() {
        assert_eq!(device_block(64, 48_000, 48_000), 64);
        assert_eq!(device_block(64, 48_000, 96_000), 128);
        assert_eq!(device_block(256, 48_000, 44_100), 235);
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
    fn monitor_worker_runs_at_wall_clock() {
        use super::super::dag::build_output_graph;
        use crate::audio::effects::EffectRegistry;
        use crate::audio::graph::{EdgeSpec, GraphSpec, NodeKind, NodeSpec, ValidGraph};
        use std::collections::HashMap;

        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
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
            super::super::dag::TIMER_BLOCK_FRAMES,
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
            &HashMap::new(),
            &crate::audio::pipeline::cushion::DepthMemos::default(),
        )
        .expect("build");

        let (recorder, _ctrl) = start_monitor_worker(built.graph).expect("spawn monitor");
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while built.output.blocks.load(Ordering::Relaxed) == 0
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
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

    #[test]
    fn the_monitor_runs_in_real_time_at_any_block() {
        use super::super::dag::build_output_graph;
        use crate::audio::effects::EffectRegistry;
        use crate::audio::graph::{EdgeSpec, GraphSpec, NodeKind, NodeSpec, ValidGraph};
        use std::collections::HashMap;

        for block in [32, 1024] {
            let g = GraphSpec {
                sample_rate: None,
                buffer_frames: None,
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
                block,
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
                &HashMap::new(),
                &crate::audio::pipeline::cushion::DepthMemos::default(),
            )
            .expect("build");
            let (recorder, _ctrl) = start_monitor_worker(built.graph).expect("spawn monitor");
            std::thread::sleep(Duration::from_millis(100));
            let from = built.output.blocks.load(Ordering::Relaxed);
            let started = std::time::Instant::now();
            std::thread::sleep(Duration::from_millis(500));
            let blocks = built.output.blocks.load(Ordering::Relaxed) - from;
            let secs = started.elapsed().as_secs_f64();
            drop(recorder);
            // A monitor that falls behind real time stops draining its rings,
            // and a shared source then starves every other output.
            let rate = blocks as f64 * block as f64 / secs;
            assert!(
                (40_000.0..56_000.0).contains(&rate),
                "{block}-frame monitor ran at {rate:.0} frames/s"
            );
        }
    }
}
