//! Per-channel encode/decode over the direct-IP transport. Operates on 48 kHz
//! interleaved stereo; graph-rate resampling happens at the node layer.

use tracing::warn;

use crate::audio::graph::OpusApplication;

use super::packet::{
    pcm_f32_decode, pcm_f32_encode, pcm_i16_decode, pcm_i16_encode, Format, MAX_PAYLOAD,
};
use super::{OPUS_FRAME_SIZES, OPUS_MAX_PACKET_SAMPLES, SR};

/// Samples one packet of `format` carries when the sender runs
/// `block_frames` blocks: one block per packet, so a block goes out as soon
/// as it exists. PCM is capped by the datagram size; Opus takes the largest
/// frame it can encode that fits in a block, 2.5 ms at the least. Fixed for
/// the stream, which is what makes a packet's `seq` a position on the
/// source's timeline.
pub fn packet_samples(format: Format, block_frames: usize) -> usize {
    let bytes = match format {
        Format::PcmF32 => 4,
        Format::PcmI16 => 2,
        Format::Opus => {
            return OPUS_FRAME_SIZES
                .into_iter()
                .rev()
                .find(|&f| f <= block_frames)
                .unwrap_or(OPUS_FRAME_SIZES[0]);
        }
    };
    block_frames.clamp(1, MAX_PAYLOAD / bytes)
}

/// The codec's own name for an application setting.
pub fn opus_application(app: OpusApplication) -> opus::Application {
    match app {
        OpusApplication::Voip => opus::Application::Voip,
        OpusApplication::Audio => opus::Application::Audio,
        OpusApplication::LowDelay => opus::Application::LowDelay,
    }
}

/// How the packet header names an application setting.
pub fn opus_application_byte(app: OpusApplication) -> u8 {
    match app {
        OpusApplication::Voip => 1,
        OpusApplication::Audio => 2,
        OpusApplication::LowDelay => 3,
    }
}

pub struct ChannelEncoder {
    format: Format,
    chunk: usize,
    opus: Option<opus::Encoder>,
    acc: Vec<f32>,
    scratch: Vec<u8>,
}

impl ChannelEncoder {
    pub fn new(
        format: Format,
        bitrate: u32,
        application: opus::Application,
        block_frames: usize,
    ) -> Self {
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
            chunk: packet_samples(format, block_frames),
            opus,
            acc: Vec::new(),
            scratch: vec![0u8; 4096],
        }
    }

    /// Accumulates 48 kHz interleaved input and calls `emit` with each ready
    /// payload (packet body, without header).
    pub fn push(&mut self, samples: &[f32], mut emit: impl FnMut(&[u8])) {
        self.acc.extend_from_slice(samples);
        let chunk = self.chunk;
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
    // Samples the last packet carried. The sender's block (and so its packet
    // size) need not match ours, so concealment follows what this stream
    // actually carries; nothing is owed before the first packet says.
    last_chunk: usize,
}

impl ChannelDecoder {
    pub fn new() -> Self {
        let opus = opus::Decoder::new(SR, opus::Channels::Mono)
            .map_err(|e| warn!(error = %e, "opus decoder init failed"))
            .ok();
        Self {
            opus,
            pcm: vec![0.0; OPUS_MAX_PACKET_SAMPLES],
            last_chunk: 0,
        }
    }

    /// Append concealment for `packets` lost packets: exactly what they would
    /// have carried, so the channel keeps its place on the source's timeline.
    /// Opus extrapolates from decoder state; raw PCM has no codec PLC and gets
    /// silence (the playback side fades across the join).
    pub fn conceal_packets(&mut self, format: Format, packets: u16, out: &mut Vec<f32>) {
        let chunk = self.last_chunk;
        let want = out.len() + chunk * packets as usize;
        if format == Format::Opus && chunk > 0 {
            for _ in 0..packets {
                let Some(dec) = self.opus.as_mut() else { break };
                // A lost packet's length is the buffer handed in.
                match dec.decode_float(&[], &mut self.pcm[..chunk], false) {
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

    /// Samples each packet of this stream carries; 0 until one has said.
    pub fn packet_samples(&self) -> usize {
        self.last_chunk
    }

    /// Decodes one payload into 48 kHz interleaved samples appended to `out`.
    pub fn decode(&mut self, format: Format, payload: &[u8], out: &mut Vec<f32>) {
        let before = out.len();
        match format {
            Format::Opus => {
                if let Some(dec) = self.opus.as_mut() {
                    // Read from the packet itself, so a packet that fails to
                    // decode still says what it owes the timeline.
                    if let Ok(n) = dec.get_nb_samples(payload) {
                        self.last_chunk = n;
                    }
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
            0 => out.resize(before + self.last_chunk, 0.0),
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

    const BLOCK: usize = 256;
    const OPUS_FRAME: usize = 960;

    #[test]
    fn a_packet_carries_one_engine_block() {
        assert_eq!(packet_samples(Format::PcmF32, 32), 32);
        assert_eq!(packet_samples(Format::PcmI16, 256), 256);
        assert_eq!(packet_samples(Format::PcmF32, 2_048), MAX_PAYLOAD / 4);
        assert_eq!(packet_samples(Format::Opus, 32), 120);
        assert_eq!(packet_samples(Format::Opus, 256), 240);
        assert_eq!(packet_samples(Format::Opus, 1_024), 960);
    }

    #[test]
    fn pcm_encoder_emits_fixed_size_payloads() {
        let (sink, emit) = collect();
        let mut enc = ChannelEncoder::new(Format::PcmF32, 96_000, opus::Application::Audio, BLOCK);
        // One chunk plus a little slack: exactly one full packet leaves.
        let chunk = packet_samples(Format::PcmF32, BLOCK);
        enc.push(&vec![0.25f32; chunk + 3], emit);
        let packets = sink.borrow();
        assert_eq!(packets.len(), 1, "one full chunk leaves the accumulator");
        assert_eq!(packets[0].len(), chunk * 4);
    }

    #[test]
    fn pcm_i16_encoder_roundtrips_through_decoder() {
        let (sink, emit) = collect();
        let mut enc = ChannelEncoder::new(Format::PcmI16, 0, opus::Application::Audio, BLOCK);
        let chunk = packet_samples(Format::PcmI16, BLOCK);
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
        let mut enc =
            ChannelEncoder::new(Format::Opus, 96_000, opus::Application::Audio, OPUS_FRAME);
        // Feed 4 chunks of a sine; each becomes one opus packet.
        let input: Vec<f32> = (0..OPUS_FRAME * 3)
            .map(|i| 0.5 * (i as f32 * 440.0 * std::f32::consts::TAU / SR as f32).sin())
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
        let mut out = Vec::new();
        dec.conceal_packets(Format::PcmF32, 3, &mut out);
        assert!(
            out.is_empty(),
            "nothing is owed before a packet says its size"
        );
        let mut payload = Vec::new();
        pcm_f32_encode(&[1.0; 100], &mut payload);
        dec.decode(Format::PcmF32, &payload, &mut out);
        dec.conceal_packets(Format::PcmF32, 3, &mut out);
        // PCM concealment is silence, but the timeline keeps its place.
        assert_eq!(out.len(), 100 + 3 * 100);
        assert_eq!(out[0], 1.0, "existing content untouched");
        assert_eq!(out[out.len() - 1], 0.0);
    }

    #[test]
    fn a_short_opus_frame_is_concealed_at_its_own_length() {
        let (sink, emit) = collect();
        let mut enc = ChannelEncoder::new(Format::Opus, 96_000, opus::Application::Audio, 32);
        enc.push(&[0.1; 120], emit);
        let mut dec = ChannelDecoder::new();
        let mut out = Vec::new();
        dec.decode(Format::Opus, &sink.borrow()[0], &mut out);
        assert_eq!(out.len(), 120);
        dec.conceal_packets(Format::Opus, 2, &mut out);
        assert_eq!(out.len(), 3 * 120);
    }

    #[test]
    fn conceal_opus_produces_extrapolation() {
        // Encode a tone, decode one packet, then conceal: opus PLC must
        // return something non-silent.
        let (sink, emit) = collect();
        let mut enc =
            ChannelEncoder::new(Format::Opus, 96_000, opus::Application::Audio, OPUS_FRAME);
        let input: Vec<f32> = (0..OPUS_FRAME * 2)
            .map(|i| 0.5 * (i as f32 * 440.0 * std::f32::consts::TAU / SR as f32).sin())
            .collect();
        enc.push(&input, emit);
        let mut dec = ChannelDecoder::new();
        let mut out = Vec::new();
        dec.decode(Format::Opus, &sink.borrow()[0], &mut out);
        let before = out.len();
        dec.conceal_packets(Format::Opus, 2, &mut out);
        assert_eq!(out.len(), before + 2 * OPUS_FRAME);
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
