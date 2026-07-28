use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tracing::warn;

use webrtc::data_channel::RTCDataChannel;

use crate::audio::graph::OpusApplication;
use crate::audio::netaudio::codec::{ChannelDecoder, ChannelEncoder};
use crate::audio::netaudio::packet::{self, Format};
use crate::audio::resample::MultiResampler;
use crate::audio::stream_recv::broadcast_push;

use super::session::{PeerChannel, WebRtcSession};
use super::{OPUS_SR, RESAMPLE_CHUNK};

/// Per-channel encode state: resample the graph rate to 48 kHz, then hand off
/// to a format-agnostic `ChannelEncoder` (Opus or raw PCM). The encoder is
/// rebuilt when the UI switches codec.
struct ChannelEnc {
    resampler: Option<MultiResampler>,
    resampler_sr: u32,
    in_acc: Vec<f32>,
    out_acc: Vec<f32>,
    encoder: ChannelEncoder,
    format: Format,
    bitrate: u32,
    application: opus::Application,
}

impl ChannelEnc {
    fn new(format: Format, bitrate: u32, application: opus::Application) -> Self {
        Self {
            resampler: None,
            resampler_sr: 0,
            in_acc: Vec::new(),
            out_acc: Vec::new(),
            encoder: ChannelEncoder::new(format, bitrate, application),
            format,
            bitrate,
            application,
        }
    }

    fn ensure_format(&mut self, format: Format) {
        if format != self.format {
            self.format = format;
            self.encoder = ChannelEncoder::new(format, self.bitrate, self.application);
        }
    }

    fn ensure_resampler(&mut self, sr: u32) {
        if sr == self.resampler_sr {
            return;
        }
        self.resampler_sr = sr;
        self.resampler = if sr == OPUS_SR {
            None
        } else {
            match MultiResampler::new(sr, OPUS_SR, RESAMPLE_CHUNK, 2) {
                Ok(r) => Some(r),
                Err(e) => {
                    warn!(error = %e, "encode resampler init failed");
                    None
                }
            }
        };
        self.in_acc.clear();
        self.out_acc.clear();
    }

    fn resample(&mut self) {
        match self.resampler.as_mut() {
            Some(r) => {
                let need = r.chunk_in() * 2;
                let mut off = 0;
                while self.in_acc.len() - off >= need {
                    if r.process_chunk(&self.in_acc[off..off + need], &mut self.out_acc).is_err() {
                        break;
                    }
                    off += need;
                }
                self.in_acc.drain(..off);
            }
            None => {
                self.out_acc.append(&mut self.in_acc);
            }
        }
    }
}

pub fn spawn_encode_task(session: Arc<WebRtcSession>) {
    let bitrate = session.opus_bitrate;
    let application = match session.opus_application {
        OpusApplication::Voip => opus::Application::Voip,
        OpusApplication::Audio => opus::Application::Audio,
        OpusApplication::LowDelay => opus::Application::LowDelay,
    };

    tauri::async_runtime::spawn(async move {
        let mut encs: Vec<ChannelEnc> = Vec::new();
        let mut seqs: Vec<u16> = Vec::new();
        let mut seen_gen = u64::MAX;
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            let sr = session.output_sr.load(Ordering::Relaxed);
            let format = Format::from_byte(session.codec.load(Ordering::Relaxed))
                .unwrap_or(Format::Opus);

            // Drain each channel's send ring under the lock, then release it
            // before the async resample/encode/send work.
            {
                let mut cons = session.send_consumers.lock().unwrap();
                // Each encoder holds a partial Opus frame and a resampler tail;
                // keeping them across a ring swap would leave a channel added
                // now offset from its siblings by whatever they had buffered.
                let gen = session.send_gen.load(Ordering::SeqCst);
                if gen != seen_gen {
                    seen_gen = gen;
                    encs.clear();
                    seqs.clear();
                }
                while encs.len() < cons.len() {
                    encs.push(ChannelEnc::new(format, bitrate, application));
                    seqs.push(0);
                }
                encs.truncate(cons.len());
                seqs.truncate(cons.len());
                for (i, c) in cons.iter_mut().enumerate() {
                    let take = c.slots();
                    if take > 0 {
                        if let Ok(chunk) = c.read_chunk(take) {
                            let (a, b) = chunk.as_slices();
                            encs[i].in_acc.extend_from_slice(a);
                            encs[i].in_acc.extend_from_slice(b);
                            chunk.commit_all();
                        }
                    }
                }
            }

            for (i, enc) in encs.iter_mut().enumerate() {
                enc.ensure_format(format);
                enc.ensure_resampler(sr);
                enc.resample();
                let channel = i as u8;
                let seq = &mut seqs[i];
                let mut frames: Vec<Bytes> = Vec::new();
                enc.encoder.push(&enc.out_acc, |payload| {
                    let mut d = Vec::with_capacity(packet::HEADER_LEN + payload.len());
                    packet::write_header(&mut d, format, channel, *seq);
                    *seq = seq.wrapping_add(1);
                    d.extend_from_slice(payload);
                    frames.push(Bytes::copy_from_slice(&d));
                });
                enc.out_acc.clear();
                for b in frames {
                    send_to_peers(&b, &session).await;
                }
            }
        }
    });
}

async fn send_to_peers(data: &Bytes, session: &Arc<WebRtcSession>) {
    use webrtc::data_channel::data_channel_state::RTCDataChannelState;
    // Collect DCs first to avoid holding the MutexGuard across .await. Skip
    // channels that aren't open (connecting/closing) to avoid per-frame errors.
    let dcs: Vec<(String, Arc<RTCDataChannel>)> = {
        let peers = session.peers.lock().await;
        peers
            .values()
            .filter(|p| !p.muted.load(Ordering::Relaxed))
            .filter_map(|p| p.dc.lock().unwrap().clone().map(|d| (p.peer_id.clone(), d)))
            .filter(|(_, d)| d.ready_state() == RTCDataChannelState::Open)
            .collect()
    };
    for (peer_id, dc) in dcs {
        if let Err(e) = dc.send(data).await {
            warn!(peer = %peer_id, error = %e, "send failed");
        }
    }
}

/// A received packet is self-describing: `[format, channel, seq_be, ...payload]`
/// (same wire format as the direct-IP transport). Receive state for a channel is
/// created on its first packet, so peers need not agree on how many channels
/// each sends, nor on the codec.
pub async fn decode_and_write(data: Bytes, session: &Arc<WebRtcSession>, peer_id: &str) {
    let Some(pkt) = packet::parse(&data) else { return };
    let format = pkt.format;
    let channel = pkt.channel;
    let seq = pkt.seq;
    let payload = data.slice(packet::HEADER_LEN..);

    let peer = {
        let peers = session.peers.lock().await;
        peers.get(peer_id).cloned()
    };
    let Some(peer) = peer else { return };
    if peer.muted.load(Ordering::Relaxed) {
        return;
    }

    let ch = {
        let mut chans = peer.channels.lock().unwrap();
        if let Some(c) = chans.get(&channel) {
            c.clone()
        } else {
            let display = peer.display_id.lock().unwrap().clone();
            let broadcast = session.attach_channel(display, channel);
            let c = Arc::new(PeerChannel {
                decoder: std::sync::Mutex::new(ChannelDecoder::new()),
                broadcast,
                last_seq: std::sync::Mutex::new(None),
            });
            chans.insert(channel, c.clone());
            c
        }
    };

    // Count gaps between consecutive seq numbers as loss (guard against
    // reorder / wrap producing an absurd jump).
    let mut gap = 0u16;
    {
        let mut last = ch.last_seq.lock().unwrap();
        if let Some(prev) = *last {
            let g = seq.wrapping_sub(prev).wrapping_sub(1);
            if g > 0 && (g as u32) < 1000 {
                peer.lost.fetch_add(g as u64, Ordering::Relaxed);
                gap = g;
            }
        }
        *last = Some(seq);
    }
    peer.packets.fetch_add(1, Ordering::Relaxed);

    // Opus conceals lost frames from decoder state; PCM has no codec PLC (the
    // playback side fades instead), so only conceal for Opus.
    if gap > 0 && format == Format::Opus {
        for _ in 0..gap.min(10) {
            let mut c: Vec<f32> = Vec::new();
            if let Ok(mut dec) = ch.decoder.lock() {
                dec.conceal(&mut c);
            }
            if !c.is_empty() {
                broadcast_push(&ch.broadcast, &c);
            }
        }
    }

    let mut pcm: Vec<f32> = Vec::new();
    {
        let Ok(mut dec) = ch.decoder.lock() else { return };
        dec.decode(format, &payload, &mut pcm);
    }
    broadcast_push(&ch.broadcast, &pcm);
}
