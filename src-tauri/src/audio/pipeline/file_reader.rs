use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use hound::SampleFormat as WavSampleFormat;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::audio::effects::{update_meter, MeterHandle};
use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

const CHUNK_FRAMES: usize = 1024;
const BACKOFF_WHEN_FULL: Duration = Duration::from_micros(200);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const PROGRESS_EVENT: &str = "audio://audio_file_progress";

/// Sentinel for `seek_to` -- non-negative values are frame indices.
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

pub(super) struct WavInfo {
    pub sample_rate: u32,
    #[allow(dead_code)]
    pub channels: u16,
    #[allow(dead_code)]
    pub total_frames: u64,
}

pub(super) fn probe_wav(path: &PathBuf) -> AppResult<WavInfo> {
    let reader = hound::WavReader::open(path)
        .map_err(|e| AppError::Stream(format!("open wav {}: {e}", path.display())))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err(AppError::Validation(format!(
            "wav {} has zero channels",
            path.display()
        )));
    }
    let total_frames = reader.duration() as u64;
    Ok(WavInfo {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        total_frames,
    })
}

pub(super) fn start_audio_file_reader(
    node_id: String,
    path: PathBuf,
    bridge: BroadcastRx,
    meter: MeterHandle,
    initial_loop: bool,
    volume: Arc<AtomicU32>,
    app: AppHandle,
) -> AppResult<AudioFileReader> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let seek_to = Arc::new(AtomicI64::new(SEEK_NONE));
    let seek_to_thread = seek_to.clone();
    let loop_enabled = Arc::new(AtomicBool::new(initial_loop));
    let loop_enabled_thread = loop_enabled.clone();
    let volume_thread = volume.clone();

    let join = thread::Builder::new()
        .name(format!("audio-file:{}", path.display()))
        .spawn(move || {
            if let Err(e) = run(
                node_id,
                &path,
                bridge,
                meter,
                &stop_thread,
                &seek_to_thread,
                &loop_enabled_thread,
                &volume_thread,
                &app,
            ) {
                warn!(path = %path.display(), error = %e, "audio file reader failed");
            }
        })
        .map_err(|e| AppError::Stream(format!("spawn audio file reader: {e}")))?;

    Ok(AudioFileReader {
        stop,
        join: Some(join),
        seek_to,
        loop_enabled,
    })
}

#[allow(clippy::too_many_arguments)]
fn run(
    node_id: String,
    path: &PathBuf,
    mut bridge: BroadcastRx,
    meter: MeterHandle,
    stop: &AtomicBool,
    seek_to: &AtomicI64,
    loop_enabled: &AtomicBool,
    volume: &AtomicU32,
    app: &AppHandle,
) -> AppResult<()> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| AppError::Stream(format!("reopen wav {}: {e}", path.display())))?;
    let spec = reader.spec();
    let src_channels = spec.channels as usize;
    let bits = spec.bits_per_sample;
    let int_max = (1i64 << (bits - 1)) as f32;
    let total_frames = reader.duration() as u64;
    let sample_rate = spec.sample_rate;

    info!(
        path = %path.display(),
        sample_rate,
        channels = src_channels,
        bits,
        format = ?spec.sample_format,
        total_frames,
        "audio file reader started"
    );

    let mut frames_played: u64 = 0;
    let mut last_progress = Instant::now();
    emit_progress(app, &node_id, frames_played, total_frames, sample_rate, false);

    let mut stereo = vec![0.0_f32; CHUNK_FRAMES * 2];
    loop {
        if stop.load(Ordering::SeqCst) {
            emit_progress(app, &node_id, frames_played, total_frames, sample_rate, true);
            return Ok(());
        }

        let pending_seek = seek_to.swap(SEEK_NONE, Ordering::SeqCst);
        if pending_seek >= 0 {
            let target = (pending_seek as u64).min(total_frames);
            if let Err(e) = reader.seek(target as u32) {
                warn!(path = %path.display(), target, error = %e, "wav seek failed");
            } else {
                frames_played = target;
                emit_progress(app, &node_id, frames_played, total_frames, sample_rate, false);
                last_progress = Instant::now();
            }
        }

        let frames_read = read_chunk(&mut reader, src_channels, bits, int_max, &mut stereo)?;
        if frames_read == 0 {
            if loop_enabled.load(Ordering::SeqCst) {
                if let Err(e) = reader.seek(0) {
                    warn!(path = %path.display(), error = %e, "wav loop seek failed");
                    emit_progress(app, &node_id, frames_played, total_frames, sample_rate, true);
                    return Ok(());
                }
                frames_played = 0;
                continue;
            }
            info!(path = %path.display(), "audio file reached end");
            emit_progress(app, &node_id, frames_played, total_frames, sample_rate, true);
            return Ok(());
        }

        let samples = &mut stereo[..frames_read * 2];
        const ONE_BITS: u32 = 0x3F80_0000;
        let vol_bits = volume.load(Ordering::Relaxed);
        if vol_bits != ONE_BITS {
            let vol = f32::from_bits(vol_bits);
            for s in samples.iter_mut() {
                *s *= vol;
            }
        }
        update_meter(&meter, samples);
        bridge.broadcast_blocking(samples, stop, BACKOFF_WHEN_FULL);
        frames_played += frames_read as u64;

        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            emit_progress(app, &node_id, frames_played, total_frames, sample_rate, false);
            last_progress = Instant::now();
        }
    }
}

fn emit_progress(
    app: &AppHandle,
    node_id: &str,
    frames: u64,
    total_frames: u64,
    sample_rate: u32,
    stopped: bool,
) {
    let _ = app.emit(
        PROGRESS_EVENT,
        json!({
            "nodeId": node_id,
            "frames": frames,
            "totalFrames": total_frames,
            "sampleRate": sample_rate,
            "stopped": stopped,
        }),
    );
}

/// Mono is duplicated, >2 channels keeps the first two.
fn read_chunk(
    reader: &mut hound::WavReader<std::io::BufReader<std::fs::File>>,
    src_channels: usize,
    bits: u16,
    int_max: f32,
    stereo: &mut [f32],
) -> AppResult<usize> {
    let want_samples = CHUNK_FRAMES * src_channels;
    let mut raw = Vec::with_capacity(want_samples);

    match reader.spec().sample_format {
        WavSampleFormat::Int => {
            for sample in reader.samples::<i32>().take(want_samples) {
                let v = sample.map_err(|e| AppError::Stream(format!("wav read: {e}")))?;
                raw.push(v as f32 / int_max);
            }
        }
        WavSampleFormat::Float => {
            // f32 wav already in [-1, 1].
            let _ = bits;
            for sample in reader.samples::<f32>().take(want_samples) {
                let v = sample.map_err(|e| AppError::Stream(format!("wav read: {e}")))?;
                raw.push(v);
            }
        }
    }

    if raw.is_empty() {
        return Ok(0);
    }

    let frames = raw.len() / src_channels;
    for f in 0..frames {
        let base = f * src_channels;
        let (l, r) = match src_channels {
            1 => (raw[base], raw[base]),
            _ => (raw[base], raw[base + 1]),
        };
        stereo[f * 2] = l;
        stereo[f * 2 + 1] = r;
    }
    Ok(frames)
}
