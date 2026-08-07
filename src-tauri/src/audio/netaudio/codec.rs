//! Per-channel encode/decode over the direct-IP transport. Operates on 48 kHz
//! interleaved stereo; graph-rate resampling happens at the node layer.

use tracing::warn;

use super::packet::{
    pcm_f32_decode, pcm_f32_encode, pcm_i16_decode, pcm_i16_encode, Format, MAX_PAYLOAD,
};
use super::{OPUS_FRAME_SAMPLES, SR};

/// Samples one packet of `format` carries. Fixed per format, which is what
/// makes a packet's `seq` a position on the source's timeline.
pub fn chunk_samples(format: Format) -> usize {
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
            match opus::Encoder::new(SR, opus::Channels::Mono, application) {
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
        Self {
            format,
            opus,
            acc: Vec::new(),
            scratch: vec![0u8; 4096],
        }
    }

    /// Accumulates 48 kHz interleaved input and calls `emit` with each ready
    /// payload (packet body, without header).
    pub fn push(&mut self, samples: &[f32], mut emit: impl FnMut(&[u8])) {
        self.acc.extend_from_slice(samples);
        let chunk = chunk_samples(self.format);
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
    // Samples the last packet decoded to. The sender's MTU (and so its PCM
    // chunk) need not match ours, so concealment follows what this stream
    // actually carries rather than what we would have sent.
    last_chunk: usize,
}

impl ChannelDecoder {
    pub fn new() -> Self {
        let opus = opus::Decoder::new(SR, opus::Channels::Mono)
            .map_err(|e| warn!(error = %e, "opus decoder init failed"))
            .ok();
        Self {
            opus,
            pcm: vec![0.0; OPUS_FRAME_SAMPLES],
            last_chunk: 0,
        }
    }

    /// Append concealment for `packets` lost packets: exactly what they would
    /// have carried, so the channel keeps its place on the source's timeline.
    /// Opus extrapolates from decoder state; raw PCM has no codec PLC and gets
    /// silence (the playback side fades across the join).
    pub fn conceal_packets(&mut self, format: Format, packets: u16, out: &mut Vec<f32>) {
        let want = out.len() + self.chunk(format) * packets as usize;
        if format == Format::Opus {
            for _ in 0..packets {
                let Some(dec) = self.opus.as_mut() else { break };
                match dec.decode_float(&[], &mut self.pcm, false) {
                    Ok(n) => out.extend_from_slice(&self.pcm[..n]),
                    Err(e) => {
                        warn!(error = %e, "opus conceal failed");
                        break;
                    }
                }
            }
        }
        // Whatever the codec declined to produce is still owed to the timeline.
        out.resize(want.max(out.len()), 0.0);
    }

    fn chunk(&self, format: Format) -> usize {
        if self.last_chunk > 0 {
            self.last_chunk
        } else {
            chunk_samples(format)
        }
    }

    /// Decodes one payload into 48 kHz interleaved samples appended to `out`.
    pub fn decode(&mut self, format: Format, payload: &[u8], out: &mut Vec<f32>) {
        let before = out.len();
        match format {
            Format::Opus => {
                if let Some(dec) = self.opus.as_mut() {
                    match dec.decode_float(payload, &mut self.pcm, false) {
                        Ok(n) => out.extend_from_slice(&self.pcm[..n]),
                        Err(e) => warn!(error = %e, "opus decode failed"),
                    }
                }
            }
            Format::PcmF32 => pcm_f32_decode(payload, out),
            Format::PcmI16 => pcm_i16_decode(payload, out),
        }
        match out.len() - before {
            // A packet the codec rejected still owes the timeline its samples.
            0 => out.resize(before + self.chunk(format), 0.0),
            n => self.last_chunk = n,
        }
    }
}
