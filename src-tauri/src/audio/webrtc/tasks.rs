use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use rtrb::{Consumer, RingBuffer};
use tracing::warn;

use webrtc::data_channel::RTCDataChannel;

use crate::audio::graph::OpusApplication;
use crate::audio::resample::StereoResampler;
use crate::audio::streams::bulk_push;

use super::session::{ChannelBroadcast, PeerChannel, WebRtcSession};
use super::{OPUS_FRAME_SAMPLES, OPUS_SR, RECV_RING, RESAMPLE_CHUNK};

/// Per-channel Opus encode state. One instance per local send channel.
struct ChannelEnc {
    encoder: Option<opus::Encoder>,
    resampler: Option<StereoResampler>,
    resampler_sr: u32,
    in_acc: Vec<f32>,
    out_acc: Vec<f32>,
    opus_buf: Vec<u8>,
    send_buf: Vec<u8>,
}

impl ChannelEnc {
    fn new(bitrate: u32, application: opus::Application) -> Self {
        let encoder = match opus::Encoder::new(OPUS_SR, opus::Channels::Stereo, application) {
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
        };
        Self {
            encoder,
            resampler: None,
            resampler_sr: 0,
            in_acc: Vec::new(),
            out_acc: Vec::new(),
            opus_buf: vec![0u8; 4096],
            send_buf: Vec::with_capacity(4097),
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
            match StereoResampler::new(sr, OPUS_SR, RESAMPLE_CHUNK) {
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

    /// Encode the frame at `off` and prefix it with the channel index.
    fn encode_frame(&mut self, channel: u8, off: usize) -> Option<Bytes> {
        let enc = self.encoder.as_mut()?;
        match enc.encode_float(&self.out_acc[off..off + OPUS_FRAME_SAMPLES], &mut self.opus_buf) {
            Ok(n) => {
                self.send_buf.clear();
                self.send_buf.push(channel);
                self.send_buf.extend_from_slice(&self.opus_buf[..n]);
                Some(Bytes::copy_from_slice(&self.send_buf))
            }
            Err(e) => {
                warn!(error = %e, "opus encode failed");
                None
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
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            let sr = session.output_sr.load(Ordering::Relaxed);

            // Drain each channel's send ring under the lock, then release it
            // before the async resample/encode/send work.
            {
                let mut cons = session.send_consumers.lock().unwrap();
                while encs.len() < cons.len() {
                    encs.push(ChannelEnc::new(bitrate, application));
                }
                encs.truncate(cons.len());
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
                enc.ensure_resampler(sr);
                enc.resample();
                // Emit every full 20 ms frame; keep the partial tail for the
                // next tick (padding it with silence would splice in a gap).
                let mut frames: Vec<Bytes> = Vec::new();
                let mut off = 0;
                while enc.out_acc.len() - off >= OPUS_FRAME_SAMPLES {
                    if let Some(b) = enc.encode_frame(i as u8, off) {
                        frames.push(b);
                    }
                    off += OPUS_FRAME_SAMPLES;
                }
                if off > 0 {
                    enc.out_acc.drain(..off);
                }
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

/// A received Opus packet is `[channel_byte, ...opus]`. Receive state for a
/// channel is created on its first packet, so peers need not agree on how many
/// channels each sends.
pub async fn decode_and_write(data: Bytes, session: &Arc<WebRtcSession>, peer_id: &str) {
    if data.len() < 2 {
        return;
    }
    let channel = data[0];
    let payload = data.slice(1..);

    let peer = {
        let peers = session.peers.lock().await;
        peers.get(peer_id).cloned()
    };
    let Some(peer) = peer else { return };
    if peer.muted.load(Ordering::Relaxed) {
        return;
    }

    let (ch, wiring) = {
        let mut chans = peer.channels.lock().unwrap();
        if let Some(c) = chans.get(&channel) {
            (c.clone(), None)
        } else {
            let decoder = match opus::Decoder::new(OPUS_SR, opus::Channels::Stereo) {
                Ok(d) => d,
                Err(e) => {
                    warn!(error = %e, "opus decoder init failed");
                    return;
                }
            };
            let (recv_prod, recv_cons) = RingBuffer::<f32>::new(RECV_RING);
            let c = Arc::new(PeerChannel {
                decoder: std::sync::Mutex::new(decoder),
                recv_producer: std::sync::Mutex::new(Some(recv_prod)),
            });
            chans.insert(channel, c.clone());
            (c, Some(recv_cons))
        }
    };

    if let Some(recv_cons) = wiring {
        let display = peer.display_id.lock().unwrap().clone();
        let broadcast = session.attach_channel(display, channel);
        spawn_peer_snapshot_task(recv_cons, broadcast);
    }

    let mut pcm = vec![0.0_f32; OPUS_FRAME_SAMPLES];
    let decoded = {
        let Ok(mut dec) = ch.decoder.lock() else { return };
        match dec.decode_float(&payload, &mut pcm, false) {
            Ok(n) => n,
            Err(e) => {
                warn!(peer = %peer_id, error = %e, "opus decode failed");
                return;
            }
        }
    };

    let mut prod_guard = ch.recv_producer.lock().unwrap();
    if let Some(prod) = prod_guard.as_mut() {
        bulk_push(prod, &pcm[..decoded * 2]);
    }
}

// Per-target-rate resample state; one per distinct bridge sample rate.
struct RateState {
    resampler: Option<StereoResampler>,
    in_acc: Vec<f32>,
    out_acc: Vec<f32>,
}

impl RateState {
    fn new(rate: u32) -> Self {
        let resampler = if rate == OPUS_SR {
            None
        } else {
            StereoResampler::new(OPUS_SR, rate, RESAMPLE_CHUNK).ok()
        };
        Self { resampler, in_acc: Vec::new(), out_acc: Vec::new() }
    }

    fn feed(&mut self, samples: &[f32]) {
        self.out_acc.clear();
        match self.resampler.as_mut() {
            Some(r) => {
                self.in_acc.extend_from_slice(samples);
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
            None => self.out_acc.extend_from_slice(samples),
        }
    }
}

// Drains the peer channel's 48 kHz receive ring and fans it out into every live
// output bridge's playback ring, resampling once per distinct bridge rate (a
// speaker output and a monitor graph can run at different rates).
pub fn spawn_peer_snapshot_task(mut consumer: Consumer<f32>, broadcast: ChannelBroadcast) {
    use std::collections::HashMap;
    tauri::async_runtime::spawn(async move {
        let mut states: HashMap<u32, RateState> = HashMap::new();
        let mut new: Vec<f32> = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            // The peer/channel was dropped (disconnect or room cancelled) -- exit.
            if consumer.is_abandoned() {
                return;
            }

            new.clear();
            let avail = consumer.slots();
            if avail > 0 {
                if let Ok(chunk) = consumer.read_chunk(avail) {
                    let (a, b) = chunk.as_slices();
                    new.extend_from_slice(a);
                    new.extend_from_slice(b);
                    chunk.commit_all();
                }
            }

            let mut producers = broadcast.lock().unwrap();
            producers.retain(|(_, p)| !p.is_abandoned());
            let rates: Vec<u32> = {
                let mut r: Vec<u32> = producers.iter().map(|(sr, _)| *sr).collect();
                r.sort_unstable();
                r.dedup();
                r
            };
            states.retain(|rate, _| rates.contains(rate));
            for rate in rates {
                states.entry(rate).or_insert_with(|| RateState::new(rate)).feed(&new);
            }
            for (sr, prod) in producers.iter_mut() {
                if let Some(st) = states.get(sr) {
                    if !st.out_acc.is_empty() {
                        bulk_push(prod, &st.out_acc);
                    }
                }
            }
        }
    });
}
