use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use rtrb::Consumer;
use tracing::warn;

use webrtc::data_channel::RTCDataChannel;

use crate::audio::graph::OpusApplication;
use crate::audio::resample::StereoResampler;

use super::session::WebRtcSession;
use super::{OPUS_FRAME_SAMPLES, OPUS_SR, RESAMPLE_CHUNK, SNAPSHOT_MAX};

pub fn spawn_encode_task(session: Arc<WebRtcSession>) {
    let bitrate = session.opus_bitrate;
    let application = match session.opus_application {
        OpusApplication::Voip => opus::Application::Voip,
        OpusApplication::Audio => opus::Application::Audio,
        OpusApplication::LowDelay => opus::Application::LowDelay,
    };

    tauri::async_runtime::spawn(async move {
        let mut encoder = match opus::Encoder::new(OPUS_SR, opus::Channels::Stereo, application) {
            Ok(e) => e,
            Err(e) => { warn!(error = %e, "opus encoder init failed"); return; }
        };
        if let Err(e) = encoder.set_bitrate(opus::Bitrate::Bits(bitrate as i32)) {
            warn!(error = %e, "set opus bitrate failed");
        }

        // Resamples graph-rate input up to 48 kHz; rebuilt if the rate changes.
        let mut resampler: Option<StereoResampler> = None;
        let mut resampler_sr = 0u32;
        let mut in_acc: Vec<f32> = Vec::new();
        let mut out_acc: Vec<f32> = Vec::new();
        let mut opus_buf = vec![0u8; 4096];
        let mut interval = tokio::time::interval(Duration::from_millis(20));

        loop {
            interval.tick().await;

            let sr = session.output_sr.load(Ordering::Relaxed);
            if sr != resampler_sr {
                resampler_sr = sr;
                resampler = if sr == OPUS_SR {
                    None
                } else {
                    match StereoResampler::new(sr, OPUS_SR, RESAMPLE_CHUNK) {
                        Ok(r) => Some(r),
                        Err(e) => { warn!(error = %e, "encode resampler init failed"); None }
                    }
                };
                in_acc.clear();
                out_acc.clear();
            }

            {
                let mut cons_guard = session.send_consumer.lock().unwrap();
                if let Some(cons) = cons_guard.as_mut() {
                    let take = cons.slots();
                    if take > 0 {
                        if let Ok(chunk) = cons.read_chunk(take) {
                            let (a, b) = chunk.as_slices();
                            in_acc.extend_from_slice(a);
                            in_acc.extend_from_slice(b);
                            chunk.commit_all();
                        }
                    }
                }
            }

            match resampler.as_mut() {
                Some(r) => {
                    let need = r.chunk_in() * 2;
                    let mut off = 0;
                    while in_acc.len() - off >= need {
                        if r.process_chunk(&in_acc[off..off + need], &mut out_acc).is_err() {
                            break;
                        }
                        off += need;
                    }
                    in_acc.drain(..off);
                }
                None => {
                    out_acc.append(&mut in_acc);
                }
            }

            // Emit every full 20 ms frame; keep the partial tail for next tick.
            // When idle, emit one silence frame so the peer keeps receiving.
            if out_acc.len() < OPUS_FRAME_SAMPLES {
                let mut pcm = vec![0.0_f32; OPUS_FRAME_SAMPLES];
                let n = out_acc.len();
                pcm[..n].copy_from_slice(&out_acc[..n]);
                out_acc.clear();
                encode_and_send(&mut encoder, &pcm, &mut opus_buf, &session).await;
            } else {
                let mut off = 0;
                while out_acc.len() - off >= OPUS_FRAME_SAMPLES {
                    encode_and_send(
                        &mut encoder,
                        &out_acc[off..off + OPUS_FRAME_SAMPLES],
                        &mut opus_buf,
                        &session,
                    )
                    .await;
                    off += OPUS_FRAME_SAMPLES;
                }
                out_acc.drain(..off);
            }
        }
    });
}

async fn encode_and_send(
    encoder: &mut opus::Encoder,
    pcm: &[f32],
    opus_buf: &mut [u8],
    session: &Arc<WebRtcSession>,
) {
    match encoder.encode_float(pcm, opus_buf) {
        Ok(n) => {
            let data = Bytes::copy_from_slice(&opus_buf[..n]);
            // Collect DCs first to avoid holding MutexGuard across .await.
            let dcs: Vec<(String, Arc<RTCDataChannel>)> = {
                let peers = session.peers.lock().await;
                peers
                    .values()
                    .filter(|p| !p.muted.load(Ordering::Relaxed))
                    .filter_map(|p| p.dc.lock().unwrap().clone().map(|d| (p.peer_id.clone(), d)))
                    .collect()
            };
            for (peer_id, dc) in dcs {
                if let Err(e) = dc.send(&data).await {
                    warn!(peer = %peer_id, error = %e, "send failed");
                }
            }
        }
        Err(e) => warn!(error = %e, "opus encode failed"),
    }
}

pub async fn decode_and_write(data: Bytes, session: &Arc<WebRtcSession>, peer_id: &str) {
    let peer = {
        let peers = session.peers.lock().await;
        peers.get(peer_id).cloned()
    };
    let Some(peer) = peer else { return };
    if peer.muted.load(Ordering::Relaxed) { return; }

    let mut pcm = vec![0.0_f32; OPUS_FRAME_SAMPLES];
    let decoded = {
        let Ok(mut dec) = peer.decoder.lock() else { return };
        match dec.decode_float(&data, &mut pcm, false) {
            Ok(n) => n,
            Err(e) => { warn!(peer = %peer_id, error = %e, "opus decode failed"); return; }
        }
    };

    let mut prod_guard = peer.recv_producer.lock().unwrap();
    if let Some(prod) = prod_guard.as_mut() {
        crate::audio::streams::bulk_push(prod, &pcm[..decoded * 2]);
    }
}

// Drains the peer's 48 kHz receive ring, resamples it down to the graph rate,
// and publishes the latest block into `recv_snapshot` for the RT bridge. On a
// brief underflow the previous snapshot is held so playback stays continuous.
pub fn spawn_peer_snapshot_task(
    mut consumer: Consumer<f32>,
    recv_snapshot: Arc<Mutex<Vec<f32>>>,
    output_sr: Arc<std::sync::atomic::AtomicU32>,
) {
    tauri::async_runtime::spawn(async move {
        let mut resampler: Option<StereoResampler> = None;
        let mut resampler_sr = 0u32;
        let mut in_acc: Vec<f32> = Vec::new();
        let mut out_acc: Vec<f32> = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        loop {
            interval.tick().await;
            // The peer was dropped (disconnect or room cancelled) -- exit.
            if consumer.is_abandoned() {
                return;
            }

            let sr = output_sr.load(Ordering::Relaxed);
            if sr != resampler_sr {
                resampler_sr = sr;
                resampler = if sr == OPUS_SR {
                    None
                } else {
                    StereoResampler::new(OPUS_SR, sr, RESAMPLE_CHUNK).ok()
                };
                in_acc.clear();
                out_acc.clear();
            }

            let avail = consumer.slots();
            if avail > 0 {
                if let Ok(chunk) = consumer.read_chunk(avail) {
                    let (a, b) = chunk.as_slices();
                    in_acc.extend_from_slice(a);
                    in_acc.extend_from_slice(b);
                    chunk.commit_all();
                }
            }

            out_acc.clear();
            match resampler.as_mut() {
                Some(r) => {
                    let need = r.chunk_in() * 2;
                    let mut off = 0;
                    while in_acc.len() - off >= need {
                        if r.process_chunk(&in_acc[off..off + need], &mut out_acc).is_err() {
                            break;
                        }
                        off += need;
                    }
                    in_acc.drain(..off);
                }
                None => {
                    std::mem::swap(&mut out_acc, &mut in_acc);
                    in_acc.clear();
                }
            }

            // Hold the previous snapshot when no new audio arrived this tick.
            if out_acc.is_empty() {
                continue;
            }
            let take = out_acc.len().min(SNAPSHOT_MAX);
            let start = out_acc.len() - take;
            if let Ok(mut snap) = recv_snapshot.try_lock() {
                snap.clear();
                snap.extend_from_slice(&out_acc[start..]);
            }
        }
    });
}
