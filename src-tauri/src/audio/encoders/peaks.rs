//! Read-only PCM peak extraction for WAV/AIFF recordings. The File Recording
//! node shows the whole file by lazy-loading min/max bins for the visible range
//! instead of keeping every sample in RAM. Only uncompressed PCM (WAV/AIFF, the
//! appendable formats) supports cheap random access; compressed formats are
//! rejected here and fall back to the live scope.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{AppError, AppResult};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePeaks {
    pub sample_rate: u32,
    pub channels: u32,
    /// Total per-channel frames actually present in the file (up to the last
    /// flush when a recording is still in progress).
    pub total_frames: u64,
    /// First frame covered by `mins`/`maxs` (clamped to the available range).
    pub start_frame: u64,
    /// `mins[c]` / `maxs[c]` hold one bin per channel; a bin covers
    /// `frames_per_bin` frames. Bins past the end of the file are zeroed.
    pub mins: Vec<Vec<f32>>,
    pub maxs: Vec<Vec<f32>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Endian {
    Le,
    Be,
}

struct Pcm {
    data_offset: u64,
    data_size: u64,
    sample_rate: u32,
    channels: u32,
    bits: u32,
    float: bool,
    endian: Endian,
}

/// Reads `bin_count` min/max bins of `frames_per_bin` frames each, starting at
/// `start_frame`, from a WAV or AIFF file. Returns the bins plus the file's
/// sample rate, channel count and total frame count so the caller can compute
/// the scroll range without a second call.
pub fn read_peaks(
    path: &Path,
    start_frame: u64,
    frames_per_bin: u32,
    bin_count: u32,
) -> AppResult<FilePeaks> {
    let mut f =
        File::open(path).map_err(|e| AppError::Stream(format!("open {}: {e}", path.display())))?;
    let pcm = parse(&mut f)?;

    let bps = (pcm.bits / 8) as u64;
    let frame_bytes = pcm.channels as u64 * bps;
    let file_len = std::fs::metadata(path)
        .map_err(|e| AppError::Stream(format!("stat {}: {e}", path.display())))?
        .len();
    let declared_frames = pcm.data_size / frame_bytes;
    let actual_bytes = file_len.saturating_sub(pcm.data_offset);
    let total_frames = (actual_bytes / frame_bytes).min(declared_frames);

    let fpb = frames_per_bin.max(1) as u64;
    let bins = bin_count as usize;
    let ch = pcm.channels as usize;

    let start = start_frame.min(total_frames);
    let want = (bins as u64).saturating_mul(fpb);
    let end = start.saturating_add(want).min(total_frames);

    let mut mins = vec![vec![f32::INFINITY; bins]; ch];
    let mut maxs = vec![vec![f32::NEG_INFINITY; bins]; ch];

    if end > start {
        let need_bytes = (end - start) * frame_bytes;
        let mut buf = vec![0u8; need_bytes as usize];
        f.seek(SeekFrom::Start(pcm.data_offset + start * frame_bytes))
            .map_err(|e| AppError::Stream(format!("seek {}: {e}", path.display())))?;
        let got = f
            .read(&mut buf)
            .map_err(|e| AppError::Stream(format!("read {}: {e}", path.display())))?;
        let frames_read = (got as u64) / frame_bytes;

        let mut off = 0usize;
        for frame in 0..frames_read {
            let bin = (frame / fpb) as usize;
            if bin >= bins {
                break;
            }
            for c in 0..ch {
                let v = decode(&buf[off..off + bps as usize], &pcm);
                if v < mins[c][bin] {
                    mins[c][bin] = v;
                }
                if v > maxs[c][bin] {
                    maxs[c][bin] = v;
                }
                off += bps as usize;
            }
        }
    }

    for c in 0..ch {
        for bin in 0..bins {
            if mins[c][bin] == f32::INFINITY {
                mins[c][bin] = 0.0;
                maxs[c][bin] = 0.0;
            }
        }
    }

    Ok(FilePeaks {
        sample_rate: pcm.sample_rate,
        channels: pcm.channels,
        total_frames,
        start_frame: start,
        mins,
        maxs,
    })
}

fn decode(bytes: &[u8], pcm: &Pcm) -> f32 {
    if pcm.float {
        return f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    match (pcm.bits, pcm.endian) {
        (16, Endian::Le) => i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0,
        (16, Endian::Be) => i16::from_be_bytes([bytes[0], bytes[1]]) as f32 / 32768.0,
        (24, Endian::Le) => {
            let v = (bytes[0] as i32) | ((bytes[1] as i32) << 8) | ((bytes[2] as i32) << 16);
            ((v << 8) >> 8) as f32 / 8388608.0
        }
        (24, Endian::Be) => {
            let v = ((bytes[0] as i32) << 16) | ((bytes[1] as i32) << 8) | (bytes[2] as i32);
            ((v << 8) >> 8) as f32 / 8388608.0
        }
        _ => 0.0,
    }
}

fn parse(f: &mut File) -> AppResult<Pcm> {
    let mut magic = [0u8; 4];
    let r = |e: std::io::Error| AppError::Stream(format!("read pcm header: {e}"));
    f.read_exact(&mut magic).map_err(r)?;
    match &magic {
        b"RIFF" => parse_wav(f),
        b"FORM" => parse_aiff(f),
        _ => Err(AppError::Validation(
            "peak reading supports only WAV and AIFF".into(),
        )),
    }
}

fn parse_wav(f: &mut File) -> AppResult<Pcm> {
    let mut u32buf = [0u8; 4];
    let mut u16buf = [0u8; 2];
    let mut id = [0u8; 4];
    let r = |e: std::io::Error| AppError::Stream(format!("read wav header: {e}"));
    f.read_exact(&mut u32buf).map_err(r)?; // riff size
    f.read_exact(&mut id).map_err(r)?;
    if &id != b"WAVE" {
        return Err(AppError::Validation("not a WAV file".into()));
    }
    let mut sample_rate = 0u32;
    let mut channels = 0u32;
    let mut bits = 0u32;
    let mut float = false;
    let mut data_offset = 0u64;
    let mut data_size = 0u64;
    loop {
        let read = f.read(&mut id).map_err(r)?;
        if read == 0 {
            break;
        }
        if read != 4 {
            return Err(AppError::Stream("truncated WAV header".into()));
        }
        f.read_exact(&mut u32buf).map_err(r)?;
        let size = u32::from_le_bytes(u32buf) as u64;
        match &id {
            b"fmt " => {
                f.read_exact(&mut u16buf).map_err(r)?;
                let tag = u16::from_le_bytes(u16buf);
                f.read_exact(&mut u16buf).map_err(r)?;
                channels = u16::from_le_bytes(u16buf) as u32;
                f.read_exact(&mut u32buf).map_err(r)?;
                sample_rate = u32::from_le_bytes(u32buf);
                f.read_exact(&mut u32buf).map_err(r)?; // byte rate
                f.read_exact(&mut u16buf).map_err(r)?; // block align
                f.read_exact(&mut u16buf).map_err(r)?;
                bits = u16::from_le_bytes(u16buf) as u32;
                float = tag == 3;
                if size > 16 {
                    f.seek(SeekFrom::Current((size - 16) as i64)).map_err(r)?;
                }
            }
            b"data" => {
                data_offset = f.stream_position().map_err(r)?;
                data_size = size;
                break;
            }
            _ => {
                f.seek(SeekFrom::Current((size + (size & 1)) as i64))
                    .map_err(r)?;
            }
        }
    }
    if channels == 0 || sample_rate == 0 || bits == 0 || data_offset == 0 {
        return Err(AppError::Validation(
            "invalid or missing WAV fmt/data chunk".into(),
        ));
    }
    Ok(Pcm {
        data_offset,
        data_size,
        sample_rate,
        channels,
        bits,
        float,
        endian: Endian::Le,
    })
}

fn parse_aiff(f: &mut File) -> AppResult<Pcm> {
    let mut u32buf = [0u8; 4];
    let mut s16buf = [0u8; 2];
    let mut id = [0u8; 4];
    let r = |e: std::io::Error| AppError::Stream(format!("read aiff header: {e}"));
    f.read_exact(&mut u32buf).map_err(r)?; // form size
    f.read_exact(&mut id).map_err(r)?;
    if &id != b"AIFF" {
        return Err(AppError::Validation("not an AIFF file".into()));
    }
    let mut sample_rate = 0u32;
    let mut channels = 0u32;
    let mut bits = 0u32;
    let mut data_offset = 0u64;
    let mut data_size = 0u64;
    loop {
        let read = f.read(&mut id).map_err(r)?;
        if read == 0 {
            break;
        }
        if read != 4 {
            return Err(AppError::Stream("truncated AIFF header".into()));
        }
        f.read_exact(&mut u32buf).map_err(r)?;
        let size = u32::from_be_bytes(u32buf) as u64;
        match &id {
            b"COMM" => {
                f.read_exact(&mut s16buf).map_err(r)?;
                channels = i16::from_be_bytes(s16buf) as u32;
                f.read_exact(&mut u32buf).map_err(r)?; // frames
                f.read_exact(&mut s16buf).map_err(r)?;
                bits = i16::from_be_bytes(s16buf) as u32;
                let mut ext = [0u8; 10];
                f.read_exact(&mut ext).map_err(r)?;
                sample_rate = extended80_to_u32(ext);
            }
            b"SSND" => {
                f.read_exact(&mut u32buf).map_err(r)?;
                let offset = u32::from_be_bytes(u32buf) as u64;
                f.read_exact(&mut u32buf).map_err(r)?; // block size
                let pos = f.stream_position().map_err(r)?;
                data_offset = pos + offset;
                data_size = size.saturating_sub(8 + offset);
                break;
            }
            _ => {
                f.seek(SeekFrom::Current((size + (size & 1)) as i64))
                    .map_err(r)?;
            }
        }
    }
    if channels == 0 || sample_rate == 0 || bits == 0 || data_offset == 0 {
        return Err(AppError::Validation(
            "invalid or missing AIFF COMM/SSND chunk".into(),
        ));
    }
    Ok(Pcm {
        data_offset,
        data_size,
        sample_rate,
        channels,
        bits,
        float: false,
        endian: Endian::Be,
    })
}

/// Inverse of AIFF's 80-bit extended sample-rate encoding, for whole-number rates.
fn extended80_to_u32(bytes: [u8; 10]) -> u32 {
    let exp = u16::from_be_bytes([bytes[0], bytes[1]]);
    let mantissa = u64::from_be_bytes([
        bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9],
    ]);
    let shift = 63i32 + 16383i32 - exp as i32;
    if shift < 0 || shift >= 64 {
        return 0;
    }
    (mantissa >> shift as u32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("splitwave_peaks_{}_{}", std::process::id(), name));
        p
    }

    #[test]
    fn reads_peaks_from_a_recording_in_progress() {
        use crate::audio::encoders::build_encoder;
        use crate::audio::graph::{RecordingFormat, WavBitDepth};
        let path = temp_path("peaks_live.wav");
        let _ = std::fs::remove_file(&path);
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
        let mut block = Vec::with_capacity(2048);
        for f in 0..1024 {
            block.push((f % 256) as f32 / 255.0);
            block.push(-((f % 256) as f32) / 255.0);
        }
        for _ in 0..96 {
            enc.write_interleaved(&block).unwrap();
        }
        enc.flush().unwrap();
        // Header sizes are patched by the periodic flush; a reader must see the
        // flushed frames while the encoder still holds the file open.
        let peaks = read_peaks(&path, 0, 64, 100).unwrap();
        assert_eq!(peaks.total_frames, 96 * 1024);
        assert!(peaks.maxs[0][0] > 0.0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reads_wav_f32_peaks() {
        use crate::audio::encoders::build_encoder;
        use crate::audio::graph::RecordingFormat;
        let path = temp_path("peaks.wav");
        let _ = std::fs::remove_file(&path);
        // 1024 frames of stereo: L ramps 0..1, R ramps 0..-1.
        let mut block = Vec::with_capacity(2048);
        for f in 0..1024 {
            block.push(f as f32 / 1023.0);
            block.push(-(f as f32) / 1023.0);
        }
        let mut enc = build_encoder(
            &path,
            48_000,
            2,
            RecordingFormat::Wav {
                bit_depth: crate::audio::graph::WavBitDepth::F32,
            },
            false,
        )
        .unwrap();
        enc.write_interleaved(&block).unwrap();
        enc.finalize().unwrap();

        let peaks = read_peaks(&path, 0, 256, 4).unwrap();
        assert_eq!(peaks.sample_rate, 48_000);
        assert_eq!(peaks.channels, 2);
        assert_eq!(peaks.total_frames, 1024);
        assert_eq!(peaks.mins.len(), 2);
        assert_eq!(peaks.mins[0].len(), 4);
        // First bin (frames 0..256): L max ≈ 255/1023, R min ≈ -255/1023.
        assert!((peaks.maxs[0][0] - 255.0 / 1023.0).abs() < 0.002);
        assert!((peaks.mins[1][0] - (-255.0 / 1023.0)).abs() < 0.002);
        // Last bin (frames 768..1024): L max ≈ 1023/1023, R min ≈ -1023/1023.
        assert!((peaks.maxs[0][3] - 1.0).abs() < 0.002);
        assert!((peaks.mins[1][3] - (-1.0)).abs() < 0.002);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reads_aiff_i16_peaks() {
        use crate::audio::encoders::build_encoder;
        use crate::audio::graph::{AiffBitDepth, RecordingFormat};
        let path = temp_path("peaks.aiff");
        let _ = std::fs::remove_file(&path);
        let mut block = Vec::with_capacity(1024);
        for f in 0..512 {
            block.push(f as f32 / 511.0); // mono ramp 0..1
        }
        let mut enc = build_encoder(
            &path,
            44_100,
            1,
            RecordingFormat::Aiff {
                bit_depth: AiffBitDepth::I16,
            },
            false,
        )
        .unwrap();
        enc.write_interleaved(&block).unwrap();
        enc.finalize().unwrap();

        let peaks = read_peaks(&path, 0, 256, 2).unwrap();
        assert_eq!(peaks.sample_rate, 44_100);
        assert_eq!(peaks.channels, 1);
        assert_eq!(peaks.total_frames, 512);
        assert!((peaks.maxs[0][1] - 1.0).abs() < 0.002);
        let _ = std::fs::remove_file(&path);
    }
}
