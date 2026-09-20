use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::json;
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{
    FormatOptions, FormatReader, SeekMode, SeekTo, SeekedTo, TrackType,
};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, Timestamp};
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

const BACKOFF_WHEN_FULL: Duration = Duration::from_micros(200);
/// Decoded audio kept queued in each subscriber ring. Well under the DSP
/// source's backlog cushion, deep enough that a scheduler hiccup on either side
/// never runs it dry.
const PACE_QUEUE_MS: usize = 120;
/// Cap on how long end-of-file waits for the queued tail to play out.
const EOF_DRAIN_MAX: Duration = Duration::from_secs(1);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const PROGRESS_EVENT: &str = "audio://audio_file_progress";
const SEEK_NONE: i64 = -1;

/// Progress payloads go to the frontend via Tauri; tests drive the reader
/// with a recording emitter instead of an `AppHandle<Wry>`.
pub(super) trait ProgressEmitter: Send + Sync + 'static {
    fn emit_progress(&self, payload: serde_json::Value);
}

impl<R: tauri::Runtime> ProgressEmitter for AppHandle<R> {
    fn emit_progress(&self, payload: serde_json::Value) {
        let _ = self.emit(PROGRESS_EVENT, payload);
    }
}

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
    pub channels: u32,
    #[allow(dead_code)]
    pub total_frames: u64,
}

pub(super) fn probe_audio_file(path: &Path) -> AppResult<AudioFileInfo> {
    let file =
        File::open(path).map_err(|e| AppError::Stream(format!("open {}: {e}", path.display())))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
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
    let channels = audio
        .channels
        .as_ref()
        .map(|c| c.count() as u32)
        .unwrap_or(2)
        .max(1);
    let mut total_frames = track.num_frames.filter(|&n| n > 0).unwrap_or_else(|| {
        track
            .duration
            .zip(track.time_base)
            .and_then(|(d, tb)| {
                tb.calc_time(Timestamp::new(d.get() as i64))
                    .map(|t| (t.as_secs_f64() * sample_rate as f64).round() as u64)
            })
            .unwrap_or(0)
    });
    if audio.codec == CODEC_ID_OPUS {
        total_frames = total_frames.saturating_sub(track.delay.unwrap_or(0) as u64);
    }
    Ok(AudioFileInfo {
        sample_rate,
        channels,
        total_frames,
    })
}

pub(super) fn start_audio_file_reader<E: ProgressEmitter>(
    node_id: String,
    path: PathBuf,
    bridge: BroadcastRx,
    initial_loop: bool,
    paused: Arc<AtomicBool>,
    app: E,
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

    Ok(AudioFileReader {
        stop,
        join: Some(join),
        seek_to,
        loop_enabled,
    })
}

struct OpenedDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    sample_rate: u32,
    channels: usize,
    total_frames: u64,
    /// A broken Xing/Info tag can report a zero frame count; symphonia's mpa
    /// demuxer then treats the track as ending at timestamp 0 and trims every
    /// packet to zero length, so decode yields silent empty buffers and the
    /// reader hits EOF immediately. Such packets are rebuilt without trim.
    fix_gapless_trim: bool,
}

/// Symphonia's default registry plus the third-party libopus Opus decoder.
/// Symphonia has no first-party Opus codec yet, so `make_audio_decoder` for an
/// Opus track would otherwise fail with "unsupported codec".
fn codec_registry() -> symphonia::core::codecs::registry::CodecRegistry {
    let mut registry = symphonia::core::codecs::registry::CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
    registry
}

fn open_decoder(path: &Path) -> AppResult<OpenedDecoder> {
    let file =
        File::open(path).map_err(|e| AppError::Stream(format!("open {}: {e}", path.display())))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| AppError::Stream(format!("unsupported format {}: {e}", path.display())))?;

    let (track_id, sample_rate, channels, mut total_frames, audio_params, fix_gapless_trim) = {
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
        // Some formats (streaming/flac) leave `num_frames` unset; fall back to
        // the track duration so the scrubber gets a real range.
        let mut total_frames = track.num_frames.filter(|&n| n > 0).unwrap_or_else(|| {
            track
                .duration
                .zip(track.time_base)
                .and_then(|(d, tb)| {
                    tb.calc_time(Timestamp::new(d.get() as i64))
                        .map(|t| (t.as_secs_f64() * sample_rate as f64).round() as u64)
                })
                .unwrap_or(0)
        });
        // symphonia-format-ogg 0.6 exposes the raw Opus granule position as
        // num_frames even though it separately reports the leading pre-skip.
        if audio.codec == CODEC_ID_OPUS {
            total_frames = total_frames.saturating_sub(track.delay.unwrap_or(0) as u64);
        }
        let channels = audio
            .channels
            .as_ref()
            .map(|c| c.count())
            .unwrap_or(2)
            .max(1);
        let audio_params = audio.clone();
        (
            track.id,
            sample_rate,
            channels,
            total_frames,
            audio_params,
            track.num_frames == Some(0),
        )
    };

    let mut decoder = codec_registry()
        .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
        .map_err(|e| AppError::Stream(format!("unsupported codec: {e}")))?;

    // Formats that omit length metadata (no num_frames, duration or time_base)
    // still need a real total for the scrubber. Seek to the end, and if that
    // can't produce one, scan the whole file decoding it.
    if total_frames == 0 {
        let time_base = format
            .tracks()
            .iter()
            .find(|t| t.id == track_id)
            .and_then(|t| t.time_base);
        if let Some(tb) = time_base {
            if let Ok(SeekedTo { actual_ts, .. }) = format.seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::MAX,
                    track_id: None,
                },
            ) {
                total_frames = tb
                    .calc_time(actual_ts)
                    .map(|t| (t.as_secs_f64() * sample_rate as f64).round() as u64)
                    .unwrap_or(0);
            }
        }
        if total_frames == 0 {
            // Last resort: count frames by decoding the file once.
            total_frames = scan_frames(&mut format, &mut decoder, track_id, fix_gapless_trim);
        }
        // Rewind to the start for playback regardless of which probe ran.
        let _ = format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: Time::ZERO,
                track_id: None,
            },
        );
        decoder.reset();
    }

    Ok(OpenedDecoder {
        format,
        decoder,
        track_id,
        sample_rate,
        channels,
        total_frames,
        fix_gapless_trim,
    })
}

fn next_packet(
    od: &mut OpenedDecoder,
) -> Result<Option<symphonia::core::packet::Packet>, SymphoniaError> {
    match od.format.next_packet()? {
        Some(p) if od.fix_gapless_trim => {
            // Drop the bogus gapless trim; keep the full block duration.
            Ok(Some(symphonia::core::packet::Packet::new(
                p.track_id,
                p.pts,
                p.block_dur(),
                p.data,
            )))
        }
        other => Ok(other),
    }
}

/// Decodes the whole file counting audio frames — the definitive total for
/// containers that expose no length metadata and whose seek can't report one.
fn scan_frames(
    format: &mut Box<dyn FormatReader>,
    decoder: &mut Box<dyn AudioDecoder>,
    track_id: u32,
    fix_gapless_trim: bool,
) -> u64 {
    let mut total = 0u64;
    loop {
        let res = format.next_packet().map(|p| {
            p.map(|p| {
                if fix_gapless_trim {
                    symphonia::core::packet::Packet::new(p.track_id, p.pts, p.block_dur(), p.data)
                } else {
                    p
                }
            })
        });
        match res {
            Ok(Some(p)) => {
                if p.track_id != track_id {
                    continue;
                }
                if let Ok(buf) = decoder.decode(&p) {
                    total += buf.frames() as u64;
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    total
}

fn do_seek(od: &mut OpenedDecoder, target_frame: u64) {
    let secs_f64 = target_frame as f64 / od.sample_rate as f64;
    let time = Time::try_from_secs_f64(secs_f64).unwrap_or(Time::ZERO);
    match od.format.seek(
        SeekMode::Coarse,
        SeekTo::Time {
            time,
            track_id: Some(od.track_id),
        },
    ) {
        Ok(_) => {}
        Err(e) => warn!("seek failed: {e}"),
    }
    od.decoder.reset();
}

/// Reopens the file into a fresh decoder. Used for restart-to-start (loop wrap,
/// stop/rewind): symphonia's isomp4 reader can't seek back to the beginning once
/// the stream has been read — `next_packet` then dies with "no atom pending
/// read" — so a fresh probe is the only reliable rewind across formats.
fn reopen_decoder(od: &mut OpenedDecoder, path: &Path) {
    match open_decoder(path) {
        Ok(fresh) => *od = fresh,
        Err(e) => warn!(path = %path.display(), error = %e, "reopen for restart failed"),
    }
}

fn run<E: ProgressEmitter>(
    node_id: String,
    path: &Path,
    mut bridge: BroadcastRx,
    stop: &AtomicBool,
    seek_to: &AtomicI64,
    loop_enabled: &AtomicBool,
    paused: &AtomicBool,
    app: &E,
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
    emit_progress(
        app,
        &node_id,
        0,
        od.total_frames,
        od.sample_rate,
        od.channels as u32,
        false,
        false,
    );

    let mut interleaved: Vec<f32> = Vec::new();
    let ch = od.channels;
    let mut block: Vec<f32> = vec![0.0f32; 4096];
    let mut last_frame: Vec<f32> = vec![0.0f32; ch];
    let mut last_paused_progress = Instant::now();

    // Playback rate follows the consumers draining these rings: decode until
    // `pace_queue` samples are buffered, then idle. The downstream ring can
    // hold a whole short file, so backpressure alone
    // doesn't pace anything; a wall-clock schedule instead drifts against the
    // audio clock (steady underruns) and turns any stall -- a graph swap
    // re-routing our bridges -- into an unpaced catch-up burst.
    let pace_queue = (od.sample_rate as usize * PACE_QUEUE_MS / 1000) * ch;

    loop {
        if stop.load(Ordering::SeqCst) {
            emit_progress(
                app,
                &node_id,
                frames_played,
                od.total_frames,
                od.sample_rate,
                od.channels as u32,
                true,
                false,
            );
            return Ok(());
        }

        if paused.load(Ordering::SeqCst) {
            let pending = seek_to.swap(SEEK_NONE, Ordering::SeqCst);
            if pending >= 0 {
                let target = clamp_frame(pending as u64, od.total_frames);
                if target == 0 {
                    reopen_decoder(&mut od, path);
                } else {
                    do_seek(&mut od, target);
                }
                frames_played = target;
            }
            if last_paused_progress.elapsed() >= PROGRESS_INTERVAL {
                emit_progress(
                    app,
                    &node_id,
                    frames_played,
                    od.total_frames,
                    od.sample_rate,
                    od.channels as u32,
                    false,
                    true,
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
            if target == 0 {
                reopen_decoder(&mut od, path);
            } else {
                do_seek(&mut od, target);
            }
            frames_played = target;
            emit_progress(
                app,
                &node_id,
                frames_played,
                od.total_frames,
                od.sample_rate,
                od.channels as u32,
                false,
                false,
            );
            last_progress = Instant::now();
        }

        // An unrouted file has no backpressure to pace against; it falls back to
        // a per-chunk wall-clock delay after the push below.
        let mut unrouted = false;
        loop {
            if stop.load(Ordering::SeqCst) || paused.load(Ordering::SeqCst) {
                break;
            }
            match bridge.max_queued() {
                Some(q) if q >= pace_queue => thread::sleep(BACKOFF_WHEN_FULL),
                Some(_) => break,
                None => {
                    unrouted = true;
                    break;
                }
            }
        }
        if stop.load(Ordering::SeqCst) || paused.load(Ordering::SeqCst) {
            continue;
        }

        let frames_decoded = decode_next(&mut od, &mut interleaved, &mut block)?;

        if frames_decoded == 0 {
            if loop_enabled.load(Ordering::SeqCst) {
                reopen_decoder(&mut od, path);
                frames_played = 0;
                // Make the wrap visible immediately instead of waiting for the
                // next 100 ms progress tick.
                emit_progress(
                    app,
                    &node_id,
                    0,
                    od.total_frames,
                    od.sample_rate,
                    od.channels as u32,
                    false,
                    false,
                );
                last_progress = Instant::now();
                continue;
            }
            // Fade out to avoid a hard click at end of file.
            const FADE_FRAMES: usize = 128;
            let mut fade_buf = vec![0.0f32; FADE_FRAMES * ch];
            for f in 0..FADE_FRAMES {
                let t = 1.0 - (f as f32 + 1.0) / FADE_FRAMES as f32;
                for c in 0..ch {
                    fade_buf[f * ch + c] = last_frame[c] * t;
                }
            }
            bridge.broadcast_blocking(&fade_buf, stop, paused, BACKOFF_WHEN_FULL);
            // Pausing discards whatever the mixer still holds (dag.rs
            // fill_block), so let the queued tail play out first.
            let deadline = Instant::now() + EOF_DRAIN_MAX;
            while Instant::now() < deadline && !stop.load(Ordering::SeqCst) {
                match bridge.max_queued() {
                    Some(q) if q > 0 => thread::sleep(BACKOFF_WHEN_FULL),
                    _ => break,
                }
            }
            info!(path = %path.display(), "audio file reached end");
            paused.store(true, Ordering::SeqCst);
            do_seek(&mut od, 0);
            frames_played = 0;
            emit_progress(
                app,
                &node_id,
                0,
                od.total_frames,
                od.sample_rate,
                od.channels as u32,
                false,
                true,
            );
            last_progress = Instant::now();
            continue;
        }

        let samples = &block[..frames_decoded * ch];
        bridge.broadcast_blocking(samples, stop, paused, BACKOFF_WHEN_FULL);
        frames_played += frames_decoded as u64;
        last_frame.copy_from_slice(&block[(frames_decoded - 1) * ch..frames_decoded * ch]);

        if unrouted {
            thread::sleep(Duration::from_secs_f64(
                frames_decoded as f64 / od.sample_rate as f64,
            ));
        }

        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            emit_progress(
                app,
                &node_id,
                frames_played,
                od.total_frames,
                od.sample_rate,
                od.channels as u32,
                false,
                false,
            );
            last_progress = Instant::now();
        }
    }
}

fn decode_next(
    od: &mut OpenedDecoder,
    interleaved: &mut Vec<f32>,
    out: &mut Vec<f32>,
) -> AppResult<usize> {
    loop {
        let packet = match next_packet(od) {
            Ok(Some(p)) => p,
            Ok(None) => return Ok(0),
            Err(SymphoniaError::ResetRequired) => {
                od.decoder.reset();
                continue;
            }
            // A WAV whose data chunk size exceeds the actual bytes ends with
            // "unexpected end of file" instead of a clean Ok(None); that's a
            // truncated/mis-declared file reaching its real end, so treat it as
            // end-of-stream rather than killing the reader thread mid-file.
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(0);
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

        let src_ch = audio_buf.spec().channels().count().max(1);
        let n_samples = audio_buf.samples_interleaved();

        interleaved.resize(n_samples, 0.0f32);
        audio_buf.copy_to_slice_interleaved(interleaved.as_mut_slice());

        // A packet may declare fewer channels than the track; pad the rest with
        // silence so the frame stride the bridge sees never changes mid-stream.
        let dst_ch = od.channels;
        if out.len() < frames * dst_ch {
            out.resize(frames * dst_ch, 0.0);
        }
        for f in 0..frames {
            let src = f * src_ch;
            let dst = f * dst_ch;
            for c in 0..dst_ch {
                out[dst + c] = if c < src_ch {
                    interleaved[src + c]
                } else {
                    0.0
                };
            }
        }

        return Ok(frames);
    }
}

fn clamp_frame(frame: u64, total: u64) -> u64 {
    if total == 0 {
        frame
    } else {
        frame.min(total)
    }
}

fn emit_progress(
    app: &impl ProgressEmitter,
    node_id: &str,
    frames: u64,
    total_frames: u64,
    sample_rate: u32,
    channels: u32,
    stopped: bool,
    paused: bool,
) {
    app.emit_progress(json!({
        "nodeId": node_id,
        "frames": frames,
        "totalFrames": total_frames,
        "sampleRate": sample_rate,
        "channels": channels,
        "stopped": stopped,
        "paused": paused,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::encoders::build_encoder;
    use crate::audio::graph::{
        AiffBitDepth, FlacBitDepth, FlacCompression, OpusApplication, RecordingFormat, WavBitDepth,
    };

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("file_reader_test_{}_{}", std::process::id(), name));
        p
    }

    #[test]
    fn open_decoder_reports_real_frame_count() {
        let path = temp_path("open.wav");
        let _ = std::fs::remove_file(&path);
        let mut block = Vec::with_capacity(4096);
        for f in 0..2048 {
            block.push((f % 10) as f32 / 10.0);
            block.push(-(f % 10) as f32 / 10.0);
        }
        let mut enc = build_encoder(
            &path,
            48_000,
            2,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32,
            },
            false,
        )
        .unwrap();
        enc.write_interleaved(&block).unwrap();
        enc.finalize().unwrap();

        let od = open_decoder(&path).unwrap();
        assert_eq!(od.total_frames, 2048);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn seek_to_start_restarts_decode_after_eof() {
        let path = temp_path("loop.wav");
        let _ = std::fs::remove_file(&path);
        let frames = 48_000;
        let mut block = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            block.push((f % 100) as f32 / 100.0);
            block.push(-((f % 100) as i32) as f32 / 100.0);
        }
        let mut enc = build_encoder(
            &path,
            48_000,
            2,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32,
            },
            false,
        )
        .unwrap();
        enc.write_interleaved(&block).unwrap();
        enc.finalize().unwrap();

        let mut od = open_decoder(&path).unwrap();
        let mut interleaved = Vec::new();
        let mut out = vec![0.0f32; 4096];
        // Decode to EOF.
        loop {
            let n = decode_next(&mut od, &mut interleaved, &mut out).unwrap();
            if n == 0 {
                break;
            }
        }
        // Rewind to the start; a loop must decode audio again, not hit EOF.
        do_seek(&mut od, 0);
        let n = decode_next(&mut od, &mut interleaved, &mut out).unwrap();
        assert!(n > 0, "seek to start after EOF did not restart decode");
        let _ = std::fs::remove_file(&path);
    }

    // Exercises the reader's contract for one format: opens the file, reports a
    // nonzero total, decodes the whole track, then restarts from the start after
    // EOF (the loop path) instead of staying at the end. The source length is
    // deliberately not codec-frame-aligned so end trimming is exercised.
    fn assert_format_roundtrip_with_channels(fmt: RecordingFormat, label: &str, ch: u16) {
        let extension = match &fmt {
            RecordingFormat::Wav { .. } => "wav",
            RecordingFormat::Aiff { .. } => "aiff",
            RecordingFormat::Flac { .. } => "flac",
            RecordingFormat::Opus { .. } => "opus",
            RecordingFormat::Mp3 { .. } => "mp3",
            #[cfg(target_os = "macos")]
            RecordingFormat::Aac { .. } => "m4a",
        };
        let path = temp_path(&format!("{label}.{extension}"));
        let _ = std::fs::remove_file(&path);
        let sample_rate = 48_000u32;
        let frames = 48_137usize;
        // AVAudioFile's AAC container duration includes the fixed encoder
        // priming plus at most one final 1024-frame packet of padding.  Keep
        // the bound absolute: it must not grow with the recording length.
        let frame_tolerance = if matches!(&fmt, RecordingFormat::Aac { .. }) {
            4_096u64
        } else {
            0
        };
        let mut block = Vec::with_capacity(frames * ch as usize);
        for f in 0..frames {
            for channel in 0..ch {
                let sample = (f % 101) as f32 / 101.0 - 0.5;
                block.push(if channel % 2 == 0 { sample } else { -sample });
            }
        }
        let mut enc = build_encoder(&path, sample_rate, ch, fmt.clone(), false).unwrap();
        enc.write_interleaved(&block).unwrap();
        enc.finalize().unwrap();

        let mut od = open_decoder(&path).unwrap_or_else(|e| panic!("{label}: open: {e}"));
        assert_eq!(od.sample_rate, sample_rate, "{label}: sample rate");
        assert_eq!(od.channels, ch as usize, "{label}: channels");

        let duration_error = od.total_frames.abs_diff(frames as u64);
        assert!(
            duration_error <= frame_tolerance,
            "{label}: metadata reports {} frames for {frames}",
            od.total_frames
        );

        let mut interleaved = Vec::new();
        let mut out = vec![0.0f32; 8192];
        let mut decoded = 0u64;
        let mut energy = 0.0f64;
        loop {
            let n = decode_next(&mut od, &mut interleaved, &mut out).unwrap();
            if n == 0 {
                break;
            }
            decoded += n as u64;
            energy += out[..n * od.channels]
                .iter()
                .map(|sample| (*sample as f64) * (*sample as f64))
                .sum::<f64>();
        }
        assert!(
            decoded.abs_diff(frames as u64) <= frame_tolerance,
            "{label}: decoded {decoded} frames for {frames}"
        );
        assert!(energy > 1.0, "{label}: decoder returned silence");

        // Loop restart: after EOF, the reader reopens the file (symphonia's isomp4
        // reader can't rewind an already-read stream), so a fresh decode must
        // yield audio again rather than staying at the end.
        reopen_decoder(&mut od, &path);
        let mut restarted_frames = 0usize;
        let mut restarted_audio = false;
        while restarted_frames < 8192 {
            let n = decode_next(&mut od, &mut interleaved, &mut out).unwrap();
            if n == 0 {
                break;
            }
            restarted_frames += n;
            restarted_audio |= out[..n * od.channels]
                .iter()
                .any(|sample| sample.abs() > 1e-4);
            if restarted_audio {
                break;
            }
        }
        assert!(
            restarted_frames > 0,
            "{label}: decode did not restart after EOF reopen"
        );
        assert!(restarted_audio, "{label}: restart decoded only silence");

        // Metadata-less fallback: a fresh scan agrees with the reported total.
        let mut od2 = open_decoder(&path).unwrap_or_else(|e| panic!("{label}: reopen: {e}"));
        let scanned = scan_frames(
            &mut od2.format,
            &mut od2.decoder,
            od2.track_id,
            od2.fix_gapless_trim,
        );
        assert!(
            scanned.abs_diff(frames as u64) <= frame_tolerance,
            "{label}: scan_frames {scanned} for {frames}"
        );

        let _ = std::fs::remove_file(&path);
    }

    fn assert_format_roundtrip(fmt: RecordingFormat, label: &str) {
        assert_format_roundtrip_with_channels(fmt, label, 2);
    }

    #[test]
    fn all_formats_open_decode_seek_loop() {
        assert_format_roundtrip(
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32,
            },
            "wav_f32",
        );
        assert_format_roundtrip(
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::I16,
            },
            "wav_i16",
        );
        assert_format_roundtrip(
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::I24,
            },
            "wav_i24",
        );
        assert_format_roundtrip(
            RecordingFormat::Aiff {
                bit_depth: AiffBitDepth::I16,
            },
            "aiff_i16",
        );
        assert_format_roundtrip(
            RecordingFormat::Aiff {
                bit_depth: AiffBitDepth::I24,
            },
            "aiff_i24",
        );
        assert_format_roundtrip(
            RecordingFormat::Flac {
                bit_depth: FlacBitDepth::I16,
                compression: FlacCompression::Default,
            },
            "flac_i16",
        );
        assert_format_roundtrip(
            RecordingFormat::Flac {
                bit_depth: FlacBitDepth::I24,
                compression: FlacCompression::Default,
            },
            "flac_i24",
        );
        assert_format_roundtrip(RecordingFormat::Mp3 { bitrate_kbps: 192 }, "mp3");
        assert_format_roundtrip(
            RecordingFormat::Opus {
                bitrate: 128_000,
                application: OpusApplication::Audio,
            },
            "opus",
        );
        #[cfg(target_os = "macos")]
        {
            assert_format_roundtrip(RecordingFormat::Aac { bitrate: 128_000 }, "aac");
            assert_format_roundtrip_with_channels(
                RecordingFormat::Aac { bitrate: 128_000 },
                "aac_mono",
                1,
            );
        }
    }

    #[test]
    fn truncated_wav_reaches_clean_eof() {
        let path = temp_path("trunc.wav");
        let _ = std::fs::remove_file(&path);
        let frames = 32_000;
        let mut block = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            block.push((f % 100) as f32 / 100.0);
            block.push(-((f % 100) as i32) as f32 / 100.0);
        }
        let mut enc = build_encoder(
            &path,
            32_000,
            2,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32,
            },
            false,
        )
        .unwrap();
        enc.write_interleaved(&block).unwrap();
        enc.finalize().unwrap();

        // Drop trailing bytes but leave the header's data-chunk size intact, so
        // symphonia reads past the real end and reports "unexpected end of file".
        let bytes = std::fs::read(&path).unwrap();
        let remove = 100_000usize;
        std::fs::write(&path, &bytes[..bytes.len() - remove]).unwrap();

        let mut od = open_decoder(&path).unwrap();
        let mut interleaved = Vec::new();
        let mut out = vec![0.0f32; 8192];
        let mut decoded = 0u64;
        loop {
            match decode_next(&mut od, &mut interleaved, &mut out) {
                Ok(0) => break,
                Ok(n) => decoded += n as u64,
                Err(e) => panic!("decode errored instead of clean EOF: {e}"),
            }
        }
        assert!(decoded > 0, "no audio decoded from truncated wav");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn probe_falls_back_to_duration_when_num_frames_missing() {
        // An MP3 without a Xing/Info tag carries no num_frames; the probe
        // falls back to the track duration for the scrubber's range.
        let path = temp_path("probe.mp3");
        let _ = std::fs::remove_file(&path);
        let frames = 48_000usize;
        let mut block = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            block.push((f % 100) as f32 / 100.0);
            block.push(0.0);
        }
        let mut enc = build_encoder(
            &path,
            48_000,
            2,
            RecordingFormat::Mp3 { bitrate_kbps: 128 },
            false,
        )
        .unwrap();
        enc.write_interleaved(&block).unwrap();
        enc.finalize().unwrap();

        let info = probe_audio_file(&path).unwrap();
        assert_eq!(info.sample_rate, 48_000);
        assert!(
            info.total_frames > 0,
            "duration fallback must yield a total"
        );
        assert!(
            (info.total_frames as i64 - frames as i64).abs() < 8_000,
            "total {} vs source {}",
            info.total_frames,
            frames
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn probe_rejects_garbage_and_missing_files() {
        assert!(probe_audio_file(Path::new("/definitely/not/here.wav")).is_err());
        let path = temp_path("garbage.dat");
        std::fs::write(&path, [0u8; 4096]).unwrap();
        assert!(probe_audio_file(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn clamp_frame_bounds_seek_targets() {
        assert_eq!(clamp_frame(10_000, 2_000), 2_000);
        assert_eq!(clamp_frame(0, 2_000), 0);
        assert_eq!(clamp_frame(500, 0), 500, "unknown total keeps the target");
        assert_eq!(clamp_frame(u64::MAX, 1), 1);
    }

    #[test]
    fn reader_streams_audio_into_the_bridge_until_eof() {
        use file_reader_test_emitter::TestEmitter;

        let path = temp_path("stream.wav");
        let frames = 12_000usize; // 250 ms of audio
        let mut block = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            block.push(((f % 40) as f32) / 40.0);
            block.push(-((f % 40) as f32) / 40.0);
        }
        let mut enc = build_encoder(
            &path,
            48_000,
            2,
            RecordingFormat::Wav {
                bit_depth: WavBitDepth::F32,
            },
            false,
        )
        .unwrap();
        enc.write_interleaved(&block).unwrap();
        enc.finalize().unwrap();

        let paused = Arc::new(AtomicBool::new(false));
        let (tx, rx) = crate::audio::input_bridge::broadcast_channel();
        let _keep = tx;
        let reader = start_audio_file_reader(
            "node".into(),
            path.clone(),
            rx,
            false,
            paused.clone(),
            TestEmitter::default(),
        )
        .expect("reader");
        // Unrouted bridge → wall-clock pacing: 250 ms of audio plays out in
        // real time, then the reader pauses itself at EOF.
        std::thread::sleep(Duration::from_millis(900));
        assert!(paused.load(Ordering::SeqCst), "reader pauses at EOF");
        drop(reader);
        let _ = std::fs::remove_file(&path);
    }
}

/// Test-only progress sink so `pipeline/mod.rs` tests can drive the reader
/// without a Tauri `AppHandle<Wry>`.
#[cfg(test)]
pub mod file_reader_test_emitter {
    use super::ProgressEmitter;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub struct TestEmitter {
        events: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    impl TestEmitter {
        pub fn events(&self) -> Arc<Mutex<Vec<serde_json::Value>>> {
            self.events.clone()
        }
    }

    impl ProgressEmitter for TestEmitter {
        fn emit_progress(&self, payload: serde_json::Value) {
            self.events.lock().unwrap().push(payload);
        }
    }
}
