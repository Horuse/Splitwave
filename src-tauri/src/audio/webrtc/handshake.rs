use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rtrb::RingBuffer;
use tracing::info;

use webrtc::peer_connection::RTCPeerConnection;

use crate::audio::graph::OpusApplication;
use crate::audio::webrtc_codec::{decode_sdp, encode_sdp};
use crate::error::{AppError, AppResult};

use super::channels::{wire_ctrl_channel, wire_data_channel, wire_peer_events};
use super::registry::{get, get_or_create};
use super::session::PeerState;
use super::tasks::spawn_peer_snapshot_task;
use super::{AUDIO_CHANNEL, OPUS_FRAME_SAMPLES, OPUS_SR, RECV_RING, STUN_URL};

/// Returns `(connection_id, compressed_offer_code)`.
/// `connection_id` is also used as the map key on both sides.
pub async fn create_offer(
    node_id: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<(String, String)> {
    let session = get_or_create(&node_id, opus_bitrate, opus_application);

    let connection_id = cuid2::create_id();
    // Initially the display ID equals the connection_id; complete_handshake
    // replaces it with the guest's own peer ID once the answer arrives.
    let display_id = Arc::new(Mutex::new(connection_id.clone()));

    let pc = Arc::new(new_peer_connection().await?);

    let dc_init = webrtc::data_channel::data_channel_init::RTCDataChannelInit {
        ordered: Some(false),
        max_retransmits: Some(0),
        ..Default::default()
    };
    let dc = pc
        .create_data_channel(AUDIO_CHANNEL, Some(dc_init.clone()))
        .await
        .map_err(|e| AppError::Stream(format!("create data channel: {e}")))?;
    let ctrl_dc = pc
        .create_data_channel("ctrl", Some(dc_init))
        .await
        .map_err(|e| AppError::Stream(format!("create ctrl channel: {e}")))?;

    let decoder = opus::Decoder::new(OPUS_SR, opus::Channels::Stereo)
        .map_err(|e| AppError::Stream(format!("opus decoder: {e}")))?;

    let peer = Arc::new(PeerState {
        peer_id: connection_id.clone(),
        pc: pc.clone(),
        dc: Mutex::new(Some(dc.clone())),
        recv_producer: Mutex::new(None),
        decoder: Mutex::new(decoder),
        recv_snapshot: Arc::new(Mutex::new(vec![0.0_f32; OPUS_FRAME_SAMPLES])),
        muted: Arc::new(AtomicBool::new(false)),
        ping_ms: Arc::new(AtomicU32::new(0)),
        display_id: display_id.clone(),
    });

    let (prod, cons) = RingBuffer::<f32>::new(RECV_RING);
    *peer.recv_producer.lock().unwrap() = Some(prod);
    spawn_peer_snapshot_task(cons, peer.recv_snapshot.clone(), session.output_sr.clone());

    wire_data_channel(dc, &session, connection_id.clone(), node_id.clone(), display_id.clone()).await;
    wire_ctrl_channel(ctrl_dc, peer.ping_ms.clone()).await;
    session.peers.lock().await.insert(connection_id.clone(), peer);
    wire_peer_events(pc.clone(), node_id.clone(), session.clone(), display_id.clone());

    let offer = pc
        .create_offer(None)
        .await
        .map_err(|e| AppError::Stream(format!("create offer: {e}")))?;

    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let done_tx = Mutex::new(Some(done_tx));
    pc.on_ice_candidate(Box::new(move |candidate| {
        if candidate.is_none() {
            if let Some(tx) = done_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
        }
        Box::pin(async {})
    }));

    pc.set_local_description(offer)
        .await
        .map_err(|e| AppError::Stream(format!("set local description: {e}")))?;

    tokio::time::timeout(Duration::from_secs(10), done_rx)
        .await
        .map_err(|_| AppError::Stream("ICE gathering timed out".into()))?
        .ok();

    let sdp = pc
        .local_description()
        .await
        .ok_or_else(|| AppError::Stream("no local description after ICE gather".into()))?
        .sdp;

    info!(
        node = %node_id,
        peer = %connection_id,
        candidates = %candidate_summary(&sdp),
        "offer ready"
    );
    let offer_code = encode_sdp(&format!("{connection_id}\n{sdp}"))?;
    Ok((connection_id, offer_code))
}

/// Returns `(guest_peer_id, compressed_answer)` for the answerer side.
/// `guest_peer_id` is the guest's own freshly-generated identity.
pub async fn accept_offer(
    node_id: String,
    offer_code: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<(String, String)> {
    let session = get_or_create(&node_id, opus_bitrate, opus_application);

    let payload = decode_sdp(&offer_code)?;
    let (connection_id, remote_sdp) = payload
        .split_once('\n')
        .ok_or_else(|| AppError::Stream("malformed offer code".into()))?;
    let connection_id = connection_id.to_string();
    let remote_sdp = remote_sdp.to_string();
    info!(
        node = %node_id,
        peer = %connection_id,
        candidates = %candidate_summary(&remote_sdp),
        "received offer"
    );

    // Generate the guest's own identity (shown in the host's peer list).
    let guest_peer_id = cuid2::create_id();
    // Guest displays the host's connection_id as the remote peer label.
    let display_id = Arc::new(Mutex::new(connection_id.clone()));

    let pc = Arc::new(new_peer_connection().await?);

    pc.on_data_channel(Box::new({
        let session = session.clone();
        let connection_id = connection_id.clone();
        let node_id = node_id.clone();
        let display_id = display_id.clone();
        move |dc| {
            let session = session.clone();
            let connection_id = connection_id.clone();
            let node_id = node_id.clone();
            let display_id = display_id.clone();
            Box::pin(async move {
                match dc.label().as_ref() {
                    "audio" => {
                        // The guest doesn't create the channel; without storing
                        // it here this side's encode task has no sink.
                        if let Some(peer) = session.peers.lock().await.get(&connection_id) {
                            *peer.dc.lock().unwrap() = Some(dc.clone());
                        }
                        wire_data_channel(dc, &session, connection_id, node_id, display_id).await;
                    }
                    "ctrl" => {
                        let ping_ms = session.peers.lock().await
                            .get(&connection_id)
                            .map(|p| p.ping_ms.clone());
                        if let Some(ping_ms) = ping_ms {
                            wire_ctrl_channel(dc, ping_ms).await;
                        }
                    }
                    _ => {}
                }
            })
        }
    }));

    let offer =
        webrtc::peer_connection::sdp::session_description::RTCSessionDescription::offer(remote_sdp)
            .map_err(|e| AppError::Stream(format!("parse offer SDP: {e}")))?;

    pc.set_remote_description(offer)
        .await
        .map_err(|e| AppError::Stream(format!("set remote description: {e}")))?;

    let answer = pc
        .create_answer(None)
        .await
        .map_err(|e| AppError::Stream(format!("create answer: {e}")))?;

    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let done_tx = Mutex::new(Some(done_tx));
    pc.on_ice_candidate(Box::new(move |candidate| {
        if candidate.is_none() {
            if let Some(tx) = done_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
        }
        Box::pin(async {})
    }));

    pc.set_local_description(answer)
        .await
        .map_err(|e| AppError::Stream(format!("set local description: {e}")))?;

    tokio::time::timeout(Duration::from_secs(10), done_rx)
        .await
        .map_err(|_| AppError::Stream("ICE gathering timed out".into()))?
        .ok();

    let sdp = pc
        .local_description()
        .await
        .ok_or_else(|| AppError::Stream("no local description after ICE gather".into()))?
        .sdp;

    info!(
        node = %node_id,
        peer = %connection_id,
        candidates = %candidate_summary(&sdp),
        "answer ready"
    );

    let decoder = opus::Decoder::new(OPUS_SR, opus::Channels::Stereo)
        .map_err(|e| AppError::Stream(format!("opus decoder: {e}")))?;

    let peer = Arc::new(PeerState {
        peer_id: connection_id.clone(),
        pc: pc.clone(),
        dc: Mutex::new(None),
        recv_producer: Mutex::new(None),
        decoder: Mutex::new(decoder),
        recv_snapshot: Arc::new(Mutex::new(vec![0.0_f32; OPUS_FRAME_SAMPLES])),
        muted: Arc::new(AtomicBool::new(false)),
        ping_ms: Arc::new(AtomicU32::new(0)),
        display_id: display_id.clone(),
    });
    let (prod, cons) = RingBuffer::<f32>::new(RECV_RING);
    *peer.recv_producer.lock().unwrap() = Some(prod);
    spawn_peer_snapshot_task(cons, peer.recv_snapshot.clone(), session.output_sr.clone());
    session.peers.lock().await.insert(connection_id.clone(), peer);
    wire_peer_events(pc, node_id.clone(), session.clone(), display_id.clone());

    // Answer carries: connection_id (map key) + guest_peer_id (shown in host UI) + sdp.
    let answer_code = encode_sdp(&format!("{connection_id}\n{guest_peer_id}\n{sdp}"))?;
    Ok((guest_peer_id, answer_code))
}

/// Host finalises the handshake after receiving the answer code from the guest.
pub async fn complete_handshake(node_id: String, answer_code: String) -> AppResult<()> {
    let session = get(&node_id)
        .ok_or_else(|| AppError::Validation(format!("no WebRTC session for {node_id}")))?;

    let payload = decode_sdp(&answer_code)?;
    let mut parts = payload.splitn(3, '\n');
    let connection_id = parts
        .next()
        .ok_or_else(|| AppError::Stream("malformed answer: missing connection_id".into()))?;
    let guest_peer_id = parts
        .next()
        .ok_or_else(|| AppError::Stream("malformed answer: missing guest_peer_id".into()))?;
    let remote_sdp = parts
        .next()
        .ok_or_else(|| AppError::Stream("malformed answer: missing sdp".into()))?;
    info!(
        node = %node_id,
        peer = %connection_id,
        candidates = %candidate_summary(remote_sdp),
        "received answer"
    );

    let peers = session.peers.lock().await;
    let peer = peers
        .get(connection_id)
        .ok_or_else(|| AppError::Validation(format!("no peer {connection_id} in session")))?;

    // Update the display ID so the host sees the guest's own identity.
    *peer.display_id.lock().unwrap() = guest_peer_id.to_string();

    let answer =
        webrtc::peer_connection::sdp::session_description::RTCSessionDescription::answer(
            remote_sdp.to_string(),
        )
        .map_err(|e| AppError::Stream(format!("parse answer SDP: {e}")))?;

    peer.pc
        .set_remote_description(answer)
        .await
        .map_err(|e| AppError::Stream(format!("set remote description: {e}")))?;

    info!(node = %node_id, peer = %connection_id, guest = %guest_peer_id, "handshake complete");
    Ok(())
}

async fn new_peer_connection() -> AppResult<RTCPeerConnection> {
    use webrtc::api::setting_engine::SettingEngine;
    use webrtc::api::APIBuilder;
    use webrtc::ice::mdns::MulticastDnsMode;
    use webrtc::ice::network_type::NetworkType;
    use webrtc::ice_transport::ice_server::RTCIceServer;
    use webrtc::peer_connection::configuration::RTCConfiguration;

    // IPv6 link-local addresses fail to bind on macOS (os error 49) and
    // STUN resolves to IPv6 when no global IPv6 route exists. Restrict to
    // UDP4 only so ICE gathering stays on IPv4.
    let mut se = SettingEngine::default();
    se.set_network_types(vec![NetworkType::Udp4]);
    // Without this, webrtc-rs obfuscates host candidates as `*.local` names
    // the remote can't resolve, so even same-LAN peers fail ICE.
    se.set_ice_multicast_dns_mode(MulticastDnsMode::Disabled);

    let api = APIBuilder::new().with_setting_engine(se).build();
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec![STUN_URL.to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };
    api.new_peer_connection(config)
        .await
        .map_err(|e| AppError::Stream(format!("new peer connection: {e}")))
}

fn candidate_summary(sdp: &str) -> String {
    let (mut host, mut srflx, mut relay, mut other, mut mdns) = (0, 0, 0, 0, 0);
    for line in sdp.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("a=candidate:") else {
            continue;
        };
        // SDP candidate: connection address is the 5th field.
        let addr = rest.split_whitespace().nth(4).unwrap_or("");
        if line.contains("typ host") {
            host += 1;
            if addr.ends_with(".local") {
                mdns += 1;
            }
        } else if line.contains("typ srflx") {
            srflx += 1;
        } else if line.contains("typ relay") {
            relay += 1;
        } else {
            other += 1;
        }
    }
    format!("host={host} mdns={mdns} srflx={srflx} relay={relay} other={other}")
}
