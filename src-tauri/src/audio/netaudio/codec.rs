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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn collect() -> (Rc<RefCell<Vec<Vec<u8>>>>, impl FnMut(&[u8])) {
        let sink: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));
        let emit = {
            let sink = sink.clone();
            move |payload: &[u8]| sink.borrow_mut().push(payload.to_vec())
        };
        (sink, emit)
    }

    #[test]
    fn chunk_samples_are_even_and_positive() {
        assert!(chunk_samples(Format::PcmF32) % 2 == 0);
        assert!(chunk_samples(Format::PcmI16) % 2 == 0);
        assert_eq!(chunk_samples(Format::Opus), OPUS_FRAME_SAMPLES);
    }

    #[test]
    fn pcm_encoder_emits_fixed_size_payloads() {
        let (sink, emit) = collect();
        let mut enc = ChannelEncoder::new(Format::PcmF32, 96_000, opus::Application::Audio);
        // One chunk plus a little slack: exactly one full packet leaves.
        let chunk = chunk_samples(Format::PcmF32);
        enc.push(&vec![0.25f32; chunk + 3], emit);
        let packets = sink.borrow();
        assert_eq!(packets.len(), 1, "one full chunk leaves the accumulator");
        assert_eq!(packets[0].len(), chunk * 4);
    }

    #[test]
    fn pcm_i16_encoder_roundtrips_through_decoder() {
        let (sink, emit) = collect();
        let mut enc = ChannelEncoder::new(Format::PcmI16, 0, opus::Application::Audio);
        let chunk = chunk_samples(Format::PcmI16);
        let input: Vec<f32> = (0..chunk)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.25 })
            .collect();
        enc.push(&input, emit);
        assert_eq!(sink.borrow().len(), 1);
        let mut dec = ChannelDecoder::new();
        let mut out = Vec::new();
        dec.decode(Format::PcmI16, &sink.borrow()[0], &mut out);
        assert_eq!(out.len(), chunk);
        for (o, i) in out.iter().zip(&input) {
            assert!((o - i).abs() < 1.0 / 32768.0 * 2.0, "{o} vs {i}");
        }
    }

    #[test]
    fn opus_encoder_decoder_roundtrip() {
        let (sink, emit) = collect();
        let mut enc = ChannelEncoder::new(Format::Opus, 96_000, opus::Application::Audio);
        // Feed 4 chunks of a sine; each becomes one opus packet.
        let input: Vec<f32> = (0..OPUS_FRAME_SAMPLES * 3)
            .map(|i| 0.5 * (i as f32 * 440.0 * 6.28 / SR as f32).sin())
            .collect();
        enc.push(&input, emit);
        assert_eq!(sink.borrow().len(), 3, "three opus packets");
        let mut dec = ChannelDecoder::new();
        let mut out = Vec::new();
        for p in sink.borrow().iter() {
            dec.decode(Format::Opus, p, &mut out);
        }
        assert!(!out.is_empty());
        assert!(out.iter().all(|s| s.is_finite()));
        // A 440 Hz sine must come back with energy, not silence.
        let rms = (out.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / out.len() as f64).sqrt();
        assert!(rms > 0.1, "decoded audio has energy: {rms}");
    }

    #[test]
    fn conceal_owes_exactly_the_lost_timeline() {
        let mut dec = ChannelDecoder::new();
        let mut out = vec![1.0f32; 10];
        dec.conceal_packets(Format::PcmF32, 3, &mut out);
        let chunk = chunk_samples(Format::PcmF32);
        // PCM concealment is silence, but the timeline keeps its place.
        assert_eq!(out.len(), 10 + 3 * chunk);
        assert_eq!(out[0], 1.0, "existing content untouched");
        assert_eq!(out[out.len() - 1], 0.0);
    }

    #[test]
    fn conceal_opus_produces_extrapolation() {
        // Encode a tone, decode one packet, then conceal: opus PLC must
        // return something non-silent.
        let (sink, emit) = collect();
        let mut enc = ChannelEncoder::new(Format::Opus, 96_000, opus::Application::Audio);
        let input: Vec<f32> = (0..OPUS_FRAME_SAMPLES * 2)
            .map(|i| 0.5 * (i as f32 * 440.0 * 6.28 / SR as f32).sin())
            .collect();
        enc.push(&input, emit);
        let mut dec = ChannelDecoder::new();
        let mut out = Vec::new();
        dec.decode(Format::Opus, &sink.borrow()[0], &mut out);
        let before = out.len();
        dec.conceal_packets(Format::Opus, 2, &mut out);
        assert_eq!(out.len(), before + 2 * OPUS_FRAME_SAMPLES);
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(
            out[before..].iter().any(|s| s.abs() > 1e-4),
            "Opus PLC returned only silence after a tone"
        );
    }

    #[test]
    fn pcm_decode_discards_only_the_trailing_partial_sample() {
        let mut dec = ChannelDecoder::new();
        let mut payload = Vec::from(1.0f32.to_le_bytes());
        payload.extend_from_slice(&(-0.5f32).to_le_bytes());
        payload.extend_from_slice(&[0xaa, 0xbb]);
        let mut out = vec![7.0];
        dec.decode(Format::PcmF32, &payload, &mut out);
        assert_eq!(out, vec![7.0, 1.0, -0.5]);
    }
}
