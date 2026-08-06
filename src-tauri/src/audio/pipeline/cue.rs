use std::io::Cursor;
use std::thread;
use std::time::Duration;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::audio::resample::MultiResampler;
use crate::error::{AppError, AppResult};

// Embedded rather than bundled: two ~13 KB clips cost nothing in the binary and
// need no per-platform resource path at playback time.
const MUTED_MP3: &[u8] = include_bytes!("../../../assets/cues/muted.mp3");
const UNMUTED_MP3: &[u8] = include_bytes!("../../../assets/cues/unmuted.mp3");
// Push-to-talk toggles far too often for the spoken clips.
const BEEP_OFF_MP3: &[u8] = include_bytes!("../../../assets/cues/beep_off.mp3");
const BEEP_ON_MP3: &[u8] = include_bytes!("../../../assets/cues/beep_on.mp3");

const RESAMPLE_CHUNK: usize = 1024;
// Slack after the clip ends so the device drains before the stream is dropped.
const DRAIN: Duration = Duration::from_millis(120);

/// Blocks for the length of the clip: the cpal stream is `!Send`, so it has to
/// be built, held and dropped on one thread. Call from a blocking task.
pub fn play(device_id: &str, muted: bool, gain: f32, beep: bool) -> AppResult<()> {
    let spec = super::output::resolve_speaker(device_id, None)?;
    let channels = spec.out_channels.max(1);
    let gain = gain.clamp(0.0, 1.0);

    let clip = match (beep, muted) {
        (false, true) => MUTED_MP3,
        (false, false) => UNMUTED_MP3,
        (true, true) => BEEP_OFF_MP3,
        (true, false) => BEEP_ON_MP3,
    };
    let mono = decode_mono(clip, spec.sample_rate)?;
    let duration = Duration::from_secs_f32(mono.len() as f32 / spec.sample_rate as f32);

    let mut pos = 0_usize;
    let render = move |out: &mut [f32], frames: usize| {
        for f in 0..frames {
            let s = mono.get(pos + f).copied().unwrap_or(0.0) * gain;
            for c in 0..channels {
                out[f * channels + c] = s;
            }
        }
        pos += frames;
    };

    #[cfg(not(target_os = "linux"))]
    let _stream = crate::audio::streams::build_output_stream(
        &spec.device,
        &spec.config,
        spec.sample_format,
        channels,
        render,
        |_| {},
    )?;

    #[cfg(target_os = "linux")]
    let _playback = {
        let mut render = render;
        crate::audio::playback::Playback::start(&spec.node_id, move |out| {
            let frames = out.len() / channels;
            render(out, frames);
            out.len()
        })?
    };

    thread::sleep(duration + DRAIN);
    Ok(())
}

/// Decodes the clip to mono at `target_rate`. The cues are short, so the whole
/// thing is materialised up front and the audio callback only reads from it.
fn decode_mono(bytes: &'static [u8], target_rate: u32) -> AppResult<Vec<f32>> {
    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| AppError::Stream(format!("probe cue: {e}")))?;

    let (track_id, src_rate, params) = {
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| AppError::Stream("no audio track in cue".into()))?;
        let audio = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| AppError::Stream("no audio codec params in cue".into()))?;
        let rate = audio
            .sample_rate
            .ok_or_else(|| AppError::Stream("unknown cue sample rate".into()))?;
        (track.id, rate, audio.clone())
    };

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| AppError::Stream(format!("cue codec: {e}")))?;

    let mut interleaved: Vec<f32> = Vec::new();
    let mut mono: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(AppError::Stream(format!("read cue packet: {e}"))),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymphoniaError::DecodeError(_)) | Err(SymphoniaError::IoError(_)) => continue,
            Err(e) => return Err(AppError::Stream(format!("decode cue: {e}"))),
        };
        let frames = buf.frames();
        if frames == 0 {
            continue;
        }
        let ch = buf.spec().channels().count().max(1);
        interleaved.resize(buf.samples_interleaved(), 0.0);
        buf.copy_to_slice_interleaved(interleaved.as_mut_slice());
        for frame in interleaved.chunks_exact(ch).take(frames) {
            mono.push(frame.iter().sum::<f32>() / ch as f32);
        }
    }

    if src_rate == target_rate {
        return Ok(mono);
    }

    let mut rs = MultiResampler::new(src_rate, target_rate, RESAMPLE_CHUNK, 1)?;
    // The tail is zero-padded to a whole chunk; trailing silence is inaudible
    // and keeps the resampler from being fed a short block.
    mono.resize(mono.len().div_ceil(RESAMPLE_CHUNK) * RESAMPLE_CHUNK, 0.0);
    let mut out = Vec::with_capacity(mono.len() * target_rate as usize / src_rate as usize + 1);
    for chunk in mono.chunks_exact(RESAMPLE_CHUNK) {
        rs.process_chunk(chunk, &mut out)?;
    }
    Ok(out)
}
