use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::json;
use symphonia::core::codecs::audio::{AudioDecoderOptions, AudioDecoder};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

const BACKOFF_WHEN_FULL: Duration = Duration::from_micros(200);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const PROGRESS_EVENT: &str = "audio://audio_file_progress";
const SEEK_NONE: i64 = -1;

pub(super) struct AudioFileReader {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    seek_to: Arc<AtomicI64>,
    loop_enabled: Arc<AtomicBool>,
}

impl AudioFileReader {
    pub(super) fn seek_to(&self) -> Arc<AtomicI64> {
        self.seek_to.clone()
    }

    pub(super) fn loop_enabled(&self) -> Arc<AtomicBool> {
        self.loop_enabled.clone()
    }
}

impl Drop for AudioFileReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub(super) struct AudioFileInfo {
    pub sample_rate: u32,
    #[allow(dead_code)]
    pub total_frames: u64,
}

pub(super) fn probe_audio_file(path: &Path) -> AppResult<AudioFileInfo> {
    let file = File::open(path)
        .map_err(|e| AppError::Stream(format!("open {}: {e}", path.display())))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| AppError::Stream(format!("unsupported format {}: {e}", path.display())))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| AppError::Stream(format!("no audio track in {}", path.display())))?;
    let audio = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| AppError::Stream("no audio codec params".into()))?;
    let sample_rate = audio
        .sample_rate
        .ok_or_else(|| AppError::Stream("unknown sample rate".into()))?;
    let total_frames = track.num_frames.unwrap_or(0);
    Ok(AudioFileInfo { sample_rate, total_frames })
}

pub(super) fn start_audio_file_reader(
    node_id: String,
    path: PathBuf,
    bridge: BroadcastRx,
    initial_loop: bool,
    paused: Arc<AtomicBool>,
    app: AppHandle,
) -> AppResult<AudioFileReader> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let seek_to = Arc::new(AtomicI64::new(SEEK_NONE));
    let seek_to_thread = seek_to.clone();
    let loop_enabled = Arc::new(AtomicBool::new(initial_loop));
    let loop_enabled_thread = loop_enabled.clone();
    let paused_thread = paused.clone();

    let join = thread::Builder::new()
        .name(format!("audio-file:{}", path.display()))
        .spawn(move || {
            if let Err(e) = run(
                node_id,
                &path,
                bridge,
                &stop_thread,
                &seek_to_thread,
                &loop_enabled_thread,
                &paused_thread,
                &app,
            ) {
                warn!(path = %path.display(), error = %e, "audio file reader failed");
            }
        })
        .map_err(|e| AppError::Stream(format!("spawn audio file reader: {e}")))?;

    Ok(AudioFileReader { stop, join: Some(join), seek_to, loop_enabled })
}

struct OpenedDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    sample_rate: u32,
    total_frames: u64,
}

fn open_decoder(path: &Path) -> AppResult<OpenedDecoder> {
    let file = File::open(path)
        .map_err(|e| AppError::Stream(format!("open {}: {e}", path.display())))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| AppError::Stream(format!("unsupported format {}: {e}", path.display())))?;

    let (track_id, sample_rate, total_frames, audio_params) = {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| AppError::Stream("no audio track".into()))?;
        let audio = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| AppError::Stream("no audio codec params".into()))?;
        let sample_rate = audio
            .sample_rate
            .ok_or_else(|| AppError::Stream("unknown sample rate".into()))?;
        let total_frames = track.num_frames.unwrap_or(0);
        let audio_params = audio.clone();
        (track.id, sample_rate, total_frames, audio_params)
    };

    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
        .map_err(|e| AppError::Stream(format!("unsupported codec: {e}")))?;

    Ok(OpenedDecoder { format, decoder, track_id, sample_rate, total_frames })
}

fn do_seek(od: &mut OpenedDecoder, target_frame: u64) {
    let secs_f64 = target_frame as f64 / od.sample_rate as f64;
    let time = Time::try_from_secs_f64(secs_f64).unwrap_or(Time::ZERO);
    match od.format.seek(SeekMode::Accurate, SeekTo::Time { time, track_id: None }) {
        Ok(_) => {}
        Err(e) => warn!("seek failed: {e}"),
    }
    od.decoder.reset();
}

fn run(
    node_id: String,
    path: &Path,
    mut bridge: BroadcastRx,
    stop: &AtomicBool,
    seek_to: &AtomicI64,
    loop_enabled: &AtomicBool,
    paused: &AtomicBool,
    app: &AppHandle,
) -> AppResult<()> {
    let mut od = open_decoder(path)?;

    info!(
        path = %path.display(),
        sample_rate = od.sample_rate,
        total_frames = od.total_frames,
        "audio file reader started"
    );

    let mut frames_played: u64 = 0;
    let mut last_progress = Instant::now();
    emit_progress(app, &node_id, 0, od.total_frames, od.sample_rate, false, false);

    let mut interleaved: Vec<f32> = Vec::new();
    let mut stereo: Vec<f32> = vec![0.0f32; 4096];
    let mut last_paused_progress = Instant::now();
    let mut last_l = 0.0f32;
    let mut last_r = 0.0f32;

    loop {
        if stop.load(Ordering::SeqCst) {
            emit_progress(
                app, &node_id, frames_played, od.total_frames, od.sample_rate, true, false,
            );
            return Ok(());
        }

        if paused.load(Ordering::SeqCst) {
            let pending = seek_to.swap(SEEK_NONE, Ordering::SeqCst);
            if pending >= 0 {
                let target = clamp_frame(pending as u64, od.total_frames);
                do_seek(&mut od, target);
                frames_played = target;
            }
            if last_paused_progress.elapsed() >= PROGRESS_INTERVAL {
                emit_progress(
                    app, &node_id, frames_played, od.total_frames, od.sample_rate, false, true,
                );
                last_paused_progress = Instant::now();
            }
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        last_paused_progress = Instant::now();

        let pending = seek_to.swap(SEEK_NONE, Ordering::SeqCst);
        if pending >= 0 {
            let target = clamp_frame(pending as u64, od.total_frames);
            do_seek(&mut od, target);
            frames_played = target;
            emit_progress(
                app, &node_id, frames_played, od.total_frames, od.sample_rate, false, false,
            );
            last_progress = Instant::now();
        }

        let frames_decoded = decode_next(&mut od, &mut interleaved, &mut stereo)?;

        if frames_decoded == 0 {
            if loop_enabled.load(Ordering::SeqCst) {
                do_seek(&mut od, 0);
                frames_played = 0;
                continue;
            }
            // Fade out to avoid a hard click at end of file.
            const FADE_FRAMES: usize = 128;
            let mut fade_buf = [0.0f32; FADE_FRAMES * 2];
            for f in 0..FADE_FRAMES {
                let t = 1.0 - (f as f32 + 1.0) / FADE_FRAMES as f32;
                fade_buf[f * 2] = last_l * t;
                fade_buf[f * 2 + 1] = last_r * t;
            }
            bridge.broadcast_blocking(&fade_buf, stop, paused, BACKOFF_WHEN_FULL);
            info!(path = %path.display(), "audio file reached end");
            paused.store(true, Ordering::SeqCst);
            do_seek(&mut od, 0);
            frames_played = 0;
            emit_progress(app, &node_id, 0, od.total_frames, od.sample_rate, false, true);
            last_progress = Instant::now();
            continue;
        }

        let samples = &stereo[..frames_decoded * 2];
        bridge.broadcast_blocking(samples, stop, paused, BACKOFF_WHEN_FULL);
        frames_played += frames_decoded as u64;
        last_l = stereo[(frames_decoded - 1) * 2];
        last_r = stereo[(frames_decoded - 1) * 2 + 1];

        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            emit_progress(
                app, &node_id, frames_played, od.total_frames, od.sample_rate, false, false,
            );
            last_progress = Instant::now();
        }
    }
}

fn decode_next(
    od: &mut OpenedDecoder,
    interleaved: &mut Vec<f32>,
    stereo: &mut Vec<f32>,
) -> AppResult<usize> {
    loop {
        let packet = match od.format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => return Ok(0),
            Err(SymphoniaError::ResetRequired) => {
                od.decoder.reset();
                continue;
            }
            Err(e) => return Err(AppError::Stream(format!("read packet: {e}"))),
        };

        if packet.track_id != od.track_id {
            continue;
        }

        let audio_buf = match od.decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(msg)) => {
                warn!("decode error (skipped): {msg}");
                continue;
            }
            Err(SymphoniaError::IoError(_)) => continue,
            Err(e) => return Err(AppError::Stream(format!("decode: {e}"))),
        };

        let frames = audio_buf.frames();
        if frames == 0 {
            continue;
        }

        let channels = audio_buf.spec().channels().count().max(1);
        let n_samples = audio_buf.samples_interleaved();

        interleaved.resize(n_samples, 0.0f32);
        audio_buf.copy_to_slice_interleaved(interleaved.as_mut_slice());

        if stereo.len() < frames * 2 {
            stereo.resize(frames * 2, 0.0);
        }

        for f in 0..frames {
            let base = f * channels;
            let (l, r) = if channels == 1 {
                (interleaved[base], interleaved[base])
            } else {
                (interleaved[base], interleaved[base + 1])
            };
            stereo[f * 2] = l;
            stereo[f * 2 + 1] = r;
        }

        return Ok(frames);
    }
}

fn clamp_frame(frame: u64, total: u64) -> u64 {
    if total == 0 { frame } else { frame.min(total) }
}

fn emit_progress(
    app: &AppHandle,
    node_id: &str,
    frames: u64,
    total_frames: u64,
    sample_rate: u32,
    stopped: bool,
    paused: bool,
) {
    let _ = app.emit(
        PROGRESS_EVENT,
        json!({
            "nodeId": node_id,
            "frames": frames,
            "totalFrames": total_frames,
            "sampleRate": sample_rate,
            "stopped": stopped,
            "paused": paused,
        }),
    );
}
