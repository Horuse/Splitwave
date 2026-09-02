//! MP3 encoder via `mp3lame-encoder` (libmp3lame). CBR with LAME tag for
//! accurate player-side duration. Sync-header per frame → a crash leaves a
//! playable file; only the LAME tag remains as placeholder (player shows
//! approximate duration from first frame).

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::mem::MaybeUninit;
use std::path::Path;

use mp3lame_encoder::{max_required_buffer_size, Builder, FlushNoGap, InterleavedPcm, MonoPcm};

use super::AudioEncoder;
use crate::error::{AppError, AppResult};

pub struct Mp3Recorder {
    encoder: mp3lame_encoder::Encoder,
    file: BufWriter<File>,
    out_buf: Vec<u8>,
    channels: u16,
}

impl Mp3Recorder {
    pub fn create(
        path: &Path,
        sample_rate: u32,
        channels: u16,
        bitrate_kbps: u32,
    ) -> AppResult<Self> {
        let mut builder =
            Builder::new().ok_or_else(|| AppError::Stream("mp3 builder init failed".into()))?;
        builder
            .set_num_channels(channels as u8)
            .map_err(|e| AppError::Stream(format!("mp3 channels: {e:?}")))?;
        builder
            .set_sample_rate(sample_rate)
            .map_err(|e| AppError::Stream(format!("mp3 sample rate: {e:?}")))?;
        builder
            .set_brate(bitrate_to_lame(bitrate_kbps))
            .map_err(|e| AppError::Stream(format!("mp3 bitrate: {e:?}")))?;
        builder
            .set_quality(mp3lame_encoder::Quality::Best)
            .map_err(|e| AppError::Stream(format!("mp3 quality: {e:?}")))?;
        builder
            .set_to_write_vbr_tag(true)
            .map_err(|e| AppError::Stream(format!("mp3 vbr tag: {e:?}")))?;
        let encoder = builder
            .build()
            .map_err(|e| AppError::Stream(format!("mp3 build: {e:?}")))?;

        let file = BufWriter::new(
            File::create(path)
                .map_err(|e| AppError::Stream(format!("create {}: {e}", path.display())))?,
        );

        Ok(Self {
            encoder,
            file,
            out_buf: Vec::with_capacity(8192),
            channels,
        })
    }

    fn ensure_capacity(&mut self, n: usize) {
        if self.out_buf.capacity() < n {
            self.out_buf.reserve(n - self.out_buf.capacity());
        }
    }
}

impl AudioEncoder for Mp3Recorder {
    fn write_interleaved(&mut self, samples: &[f32]) -> AppResult<()> {
        let frames = samples.len() / self.channels as usize;
        let needed = max_required_buffer_size(frames);
        self.out_buf.clear();
        self.ensure_capacity(needed);
        let spare: &mut [MaybeUninit<u8>] = self.out_buf.spare_capacity_mut();
        let written = if self.channels == 1 {
            self.encoder.encode(MonoPcm(samples), spare)
        } else {
            self.encoder.encode(InterleavedPcm(samples), spare)
        }
        .map_err(|e| AppError::Stream(format!("mp3 encode: {e:?}")))?;
        // SAFETY: `encode` initialised exactly `written` bytes via libmp3lame.
        unsafe { self.out_buf.set_len(written) };
        self.file
            .write_all(&self.out_buf[..written])
            .map_err(|e| AppError::Stream(format!("mp3 write: {e}")))
    }

    fn flush(&mut self) -> AppResult<()> {
        self.file
            .flush()
            .map_err(|e| AppError::Stream(format!("flush mp3: {e}")))
    }

    fn finalize(mut self: Box<Self>) -> AppResult<()> {
        self.out_buf.clear();
        self.ensure_capacity(7200);
        let spare: &mut [MaybeUninit<u8>] = self.out_buf.spare_capacity_mut();
        let written = self
            .encoder
            .flush::<FlushNoGap>(spare)
            .map_err(|e| AppError::Stream(format!("mp3 final flush: {e:?}")))?;
        unsafe { self.out_buf.set_len(written) };
        self.file
            .write_all(&self.out_buf[..written])
            .map_err(|e| AppError::Stream(format!("mp3 trailing write: {e}")))?;

        // Patch the LAME/Xing tag over the placeholder frame at file start.
        self.out_buf.clear();
        self.ensure_capacity(7200);
        let spare: &mut [MaybeUninit<u8>] = self.out_buf.spare_capacity_mut();
        if let Some(tag_size) = self.encoder.lame_tag_encode(spare) {
            unsafe { self.out_buf.set_len(tag_size.get()) };
            self.file
                .flush()
                .map_err(|e| AppError::Stream(format!("mp3 flush before tag: {e}")))?;
            let f = self.file.get_mut();
            let pos = f
                .stream_position()
                .map_err(|e| AppError::Stream(format!("mp3 stream_position: {e}")))?;
            f.seek(SeekFrom::Start(0))
                .map_err(|e| AppError::Stream(format!("mp3 seek tag: {e}")))?;
            f.write_all(&self.out_buf[..tag_size.get()])
                .map_err(|e| AppError::Stream(format!("mp3 write tag: {e}")))?;
            f.seek(SeekFrom::Start(pos))
                .map_err(|e| AppError::Stream(format!("mp3 seek end: {e}")))?;
        }

        self.file
            .flush()
            .map_err(|e| AppError::Stream(format!("mp3 finalize flush: {e}")))
    }
}

/// LAME CBR is discrete, so a custom bitrate snaps to the nearest rung.
fn bitrate_to_lame(kbps: u32) -> mp3lame_encoder::Bitrate {
    use mp3lame_encoder::Bitrate::*;
    const LADDER: [(u32, mp3lame_encoder::Bitrate); 16] = [
        (8, Kbps8),
        (16, Kbps16),
        (24, Kbps24),
        (32, Kbps32),
        (40, Kbps40),
        (48, Kbps48),
        (64, Kbps64),
        (80, Kbps80),
        (96, Kbps96),
        (112, Kbps112),
        (128, Kbps128),
        (160, Kbps160),
        (192, Kbps192),
        (224, Kbps224),
        (256, Kbps256),
        (320, Kbps320),
    ];
    let mut best = LADDER[0];
    for pair in LADDER {
        if (pair.0 as i64 - kbps as i64).abs() < (best.0 as i64 - kbps as i64).abs() {
            best = pair;
        }
    }
    best.1
}
