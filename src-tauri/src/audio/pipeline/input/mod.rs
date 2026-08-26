use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use cpal::traits::StreamTrait;
use rtrb::RingBuffer;
use tauri::AppHandle;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tracing::warn;

use crate::audio::effects::{update_meter, MeterHandle};
use crate::audio::input_bridge::{broadcast_channel, BroadcastRx};
use crate::audio::resample::MultiResampler;
use crate::audio::ENGINE_SAMPLE_RATE;
use crate::error::{AppError, AppResult};

use super::file_reader::{probe_audio_file, start_audio_file_reader, AudioFileReader};

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

pub(super) use platform::resolve_input;
use platform::start_input_stream as start_native_input_stream;

use super::dag::{RESAMPLE_CHUNK, RING_CAPACITY_FRAMES};

/// ScreenCaptureKit (macOS) and PipeWire (Linux) both deliver 48 kHz, matching
/// the device side so no resampling happens on capture delivery.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(super) const SCK_SR: u32 = 48_000;

/// RAII handle held only for its `Drop` -- stops the cpal stream, tears
/// down the capture, or signals + joins the file reader thread.
#[allow(dead_code)]
pub(super) enum InputHandle {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Cpal(cpal::Stream),
    Capture(crate::audio::capture::Capture),
    AudioFile(AudioFileReader),
    Normalized(NormalizedInput),
}

pub(super) struct NormalizedInput {
    _input: Box<InputHandle>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for NormalizedInput {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

// cpal's coreaudio backend never stops a non-default device's AudioUnit just
// because the `Stream` handle went out of scope -- a device-alive listener it
// registers internally holds another strong reference to the same stream (see
// the speaker-side `SpeakerHandle` for the full explanation), so the capture
// callback keeps broadcasting into subscriber rings nobody drains anymore.
// `pause` reaches the device through `&self` and stops it regardless.
impl Drop for InputHandle {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let InputHandle::Cpal(stream) = self {
            if let Err(e) = stream.pause() {
                warn!(error = %e, "failed to pause input stream on teardown");
            }
        }
    }
}

impl InputHandle {
    #[cfg(target_os = "macos")]
    fn tap_rate_probe(&self) -> Option<crate::audio::capture::macos_tap::TapRateProbe> {
        match self {
            InputHandle::Capture(capture) => capture.tap_rate_probe(),
            _ => None,
        }
    }
}

pub(super) enum ResolvedInput {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Cpal {
        device: cpal::Device,
        config: cpal::StreamConfig,
        sample_format: cpal::SampleFormat,
        src_channels: usize,
        sample_rate: u32,
    },
    #[cfg(target_os = "linux")]
    PwSource {
        node_id: String,
        sample_rate: u32,
    },
    SystemAudio {
        sample_rate: u32,
        // PipeWire sink-monitor capture can't exclude our own output.
        #[cfg_attr(target_os = "linux", allow(dead_code))]
        exclude_current_app: bool,
    },
    AppAudio {
        sample_rate: u32,
        bundle_id: String,
    },
    AudioFile {
        sample_rate: u32,
        channels: u32,
        path: PathBuf,
    },
}

impl ResolvedInput {
    pub(super) fn sample_rate(&self) -> u32 {
        match self {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            ResolvedInput::Cpal { sample_rate, .. } => *sample_rate,
            #[cfg(target_os = "linux")]
            ResolvedInput::PwSource { sample_rate, .. } => *sample_rate,
            ResolvedInput::SystemAudio { sample_rate, .. } => *sample_rate,
            ResolvedInput::AppAudio { sample_rate, .. } => *sample_rate,
            ResolvedInput::AudioFile { sample_rate, .. } => *sample_rate,
        }
    }

    /// Channel count this input broadcasts. Only cpal devices carry their
    /// native count; every other source path emits stereo.
    pub(super) fn native_channels(&self) -> u32 {
        match self {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            ResolvedInput::Cpal { src_channels, .. } => (*src_channels as u32).max(1),
            ResolvedInput::AudioFile { channels, .. } => (*channels).max(1),
            _ => 2,
        }
    }
}

/// Shared file probe -- both platforms resolve audio files identically.
pub(super) fn resolve_audio_file(file_path: &str) -> AppResult<ResolvedInput> {
    let path = PathBuf::from(file_path);
    let info = probe_audio_file(&path)?;
    Ok(ResolvedInput::AudioFile {
        sample_rate: info.sample_rate,
        channels: info.channels,
        path,
    })
}

/// Shared file-reader start -- both platforms drive audio files identically.
pub(super) fn start_audio_file(
    node_id: &str,
    path: PathBuf,
    bridge: BroadcastRx,
    paused: Option<Arc<AtomicBool>>,
    app: &AppHandle,
) -> AppResult<InputHandle> {
    // Loop is a runtime atomic, not in InputSpec; frontend syncs it
    // via `set_audio_file_loop` after pipeline start.
    let paused_arc = paused.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let reader = start_audio_file_reader(
        node_id.to_string(),
        path,
        bridge,
        false,
        paused_arc,
        app.clone(),
    )?;
    Ok(InputHandle::AudioFile(reader))
}

/// Capture callbacks only enqueue native-rate samples. A dedicated worker
/// normalizes each input once before the dynamic fan-out reaches the DSP graph.
pub(super) fn start_input_stream(
    node_id: &str,
    resolved: ResolvedInput,
    bridge: BroadcastRx,
    paused: Option<Arc<AtomicBool>>,
    meter: Option<MeterHandle>,
    app: &AppHandle,
) -> AppResult<InputHandle> {
    let sample_rate = resolved.sample_rate();
    let channels = resolved.native_channels() as usize;
    let (raw_producer, mut raw_consumer) =
        RingBuffer::<f32>::new(RING_CAPACITY_FRAMES * channels.max(1));
    let (mut raw_tx, raw_rx) = broadcast_channel();
    raw_tx.add(raw_producer)?;
    let input = start_native_input_stream(node_id, resolved, raw_rx, paused, None, app)?;
    #[cfg(target_os = "macos")]
    let rate_probe = input.tap_rate_probe();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let label = node_id.to_string();
    let join = thread::Builder::new()
        .name(format!("normalize:{label}"))
        .spawn(move || {
            let mut bridge = bridge;
            let mut input_buf = vec![0.0; RESAMPLE_CHUNK * channels];
            let mut native_rate = sample_rate;
            let mut resampler = if native_rate == ENGINE_SAMPLE_RATE {
                None
            } else {
                match MultiResampler::new(native_rate, ENGINE_SAMPLE_RATE, RESAMPLE_CHUNK, channels)
                {
                    Ok(resampler) => Some(resampler),
                    Err(_) => return,
                }
            };
            let mut output_buf = Vec::with_capacity(
                resampler
                    .as_ref()
                    .map(|r| r.out_max() * channels)
                    .unwrap_or(input_buf.len()),
            );
            while !stop_thread.load(std::sync::atomic::Ordering::Relaxed) {
                bridge.apply_commands();
                #[cfg(target_os = "macos")]
                if let Some(rate) = rate_probe.and_then(|probe| probe.sample_rate()) {
                    if rate == native_rate {
                        // Nothing to do; avoid perturbing the sinc state.
                    } else {
                        // Raw samples on either side of a device-rate transition
                        // cannot share a sinc state. Drop only this input's raw
                        // backlog, then rebuild the non-RT normalizer; the tap and
                        // every engine/output worker keep running.
                        let pending = raw_consumer.slots();
                        if pending > 0 {
                            if let Ok(chunk) = raw_consumer.read_chunk(pending) {
                                chunk.commit_all();
                            }
                        }
                        native_rate = rate;
                        resampler = if rate == ENGINE_SAMPLE_RATE {
                            None
                        } else {
                            MultiResampler::new(rate, ENGINE_SAMPLE_RATE, RESAMPLE_CHUNK, channels)
                                .ok()
                        };
                        output_buf = Vec::with_capacity(
                            resampler
                                .as_ref()
                                .map(|r| r.out_max() * channels)
                                .unwrap_or(input_buf.len()),
                        );
                    }
                }
                if raw_consumer.slots() < input_buf.len() {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                let Ok(chunk) = raw_consumer.read_chunk(input_buf.len()) else {
                    continue;
                };
                let (first, second) = chunk.as_slices();
                let n = first.len();
                input_buf[..n].copy_from_slice(first);
                input_buf[n..].copy_from_slice(second);
                chunk.commit_all();
                let normalized = if let Some(resampler) = &mut resampler {
                    output_buf.clear();
                    if resampler
                        .process_chunk(&input_buf, &mut output_buf)
                        .is_err()
                    {
                        break;
                    }
                    output_buf.as_slice()
                } else {
                    input_buf.as_slice()
                };
                if let Some(meter) = &meter {
                    update_meter(meter, normalized, channels);
                }
                bridge.broadcast(normalized);
            }
        })
        .map_err(|e| AppError::Stream(format!("spawn input normalizer: {e}")))?;
    Ok(InputHandle::Normalized(NormalizedInput {
        _input: Box::new(input),
        stop,
        join: Some(join),
    }))
}
