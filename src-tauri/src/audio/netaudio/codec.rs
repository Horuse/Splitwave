//! Per-channel encode/decode over the direct-IP transport. Operates on 48 kHz
//! interleaved stereo; graph-rate resampling happens at the node layer.

use tracing::warn;

use super::packet::{
    pcm_f32_decode, pcm_f32_encode, pcm_i16_decode, pcm_i16_encode, Format, MAX_PAYLOAD,
};
use super::{OPUS_FRAME_SAMPLES, SR};

/// Samples per PCM packet, sized to stay under one MTU (even for stereo).
fn pcm_chunk_samples(format: Format) -> usize {
    let bytes = match format {
        Format::PcmF32 => 4,
        Format::PcmI16 => 2,
        Format::Opus => return OPUS_FRAME_SAMPLES,
    };
    ((MAX_PAYLOAD / bytes) & !1).max(2)
}

pub struct ChannelEncoder {
    format: Format,
    opus: Option<opus::Encoder>,
    acc: Vec<f32>,
    scratch: Vec<u8>,
}

impl ChannelEncoder {
    pub fn new(format: Format, bitrate: u32, application: opus::Application) -> Self {
        let opus = if format == Format::Opus {
            match opus::Encoder::new(SR, opus::Channels::Stereo, application) {
                Ok(mut e) => {
                    if let Err(err) = e.set_bitrate(opus::Bitrate::Bits(bitrate as i32)) {
                        warn!(error = %err, "set opus bitrate failed");
                    }
                    Some(e)
                }
                Err(e) => {
                    warn!(error = %e, "opus encoder init failed");
                    None
                }
            }
        } else {
            None
        };
        Self { format, opus, acc: Vec::new(), scratch: vec![0u8; 4096] }
    }

    /// Accumulates 48 kHz interleaved input and calls `emit` with each ready
    /// payload (packet body, without header).
    pub fn push(&mut self, samples: &[f32], mut emit: impl FnMut(&[u8])) {
        self.acc.extend_from_slice(samples);
        let chunk = pcm_chunk_samples(self.format);
        let mut off = 0;
        while self.acc.len() - off >= chunk {
            let frame = &self.acc[off..off + chunk];
            match self.format {
                Format::Opus => {
                    if let Some(enc) = self.opus.as_mut() {
                        match enc.encode_float(frame, &mut self.scratch) {
                            Ok(n) => emit(&self.scratch[..n]),
                            Err(e) => warn!(error = %e, "opus encode failed"),
                        }
                    }
                }
                Format::PcmF32 => {
                    pcm_f32_encode(frame, &mut self.scratch);
                    emit(&self.scratch);
                }
                Format::PcmI16 => {
                    pcm_i16_encode(frame, &mut self.scratch);
                    emit(&self.scratch);
                }
            }
            off += chunk;
        }
        self.acc.drain(..off);
    }
}

pub struct ChannelDecoder {
    opus: Option<opus::Decoder>,
    pcm: Vec<f32>,
}

impl ChannelDecoder {
    pub fn new() -> Self {
        let opus = opus::Decoder::new(SR, opus::Channels::Stereo)
            .map_err(|e| warn!(error = %e, "opus decoder init failed"))
            .ok();
        Self { opus, pcm: vec![0.0; OPUS_FRAME_SAMPLES] }
    }

    /// Decodes one payload into 48 kHz interleaved samples appended to `out`.
    pub fn decode(&mut self, format: Format, payload: &[u8], out: &mut Vec<f32>) {
        match format {
            Format::Opus => {
                if let Some(dec) = self.opus.as_mut() {
                    match dec.decode_float(payload, &mut self.pcm, false) {
                        Ok(n) => out.extend_from_slice(&self.pcm[..n * 2]),
                        Err(e) => warn!(error = %e, "opus decode failed"),
                    }
                }
            }
            Format::PcmF32 => pcm_f32_decode(payload, out),
            Format::PcmI16 => pcm_i16_decode(payload, out),
        }
    }
}
