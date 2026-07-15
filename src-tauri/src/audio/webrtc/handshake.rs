use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::info;

use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::audio::graph::OpusApplication;
use crate::audio::webrtc_codec::{decode_sdp, encode_sdp};
use crate::error::{AppError, AppResult};

use super::channels::{wire_ctrl_channel, wire_data_channel, wire_peer_events};
use super::registry::{get, get_or_create};
use super::session::{PeerState, WebRtcSession};
use super::{AUDIO_CHANNEL, STUN_URL};

pub struct TrickleOffer {
    pub connection_id: String,
    pub sdp: String,
    pub candidates: mpsc::UnboundedReceiver<String>,
}

pub struct TrickleAnswer {
    pub guest_peer_id: String,
    pub sdp: String,
    pub candidates: mpsc::UnboundedReceiver<String>,
    pub pc: Arc<RTCPeerConnection>,
}

async fn new_host_peer(
    node_id: &str,
    session: &Arc<WebRtcSession>,
) -> AppResult<(String, Arc<RTCPeerConnection>)> {
    let connection_id = cuid2::create_id();
    // apply_answer swaps this for the guest's own peer ID once the answer arrives.
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

    let peer = Arc::new(PeerState {
        peer_id: connection_id.clone(),
        pc: pc.clone(),
        dc: Mutex::new(Some(dc.clone())),
        ctrl_dc: Mutex::new(Some(ctrl_dc.clone())),
        channels: Mutex::new(HashMap::new()),
        muted: Arc::new(AtomicBool::new(false)),
        ping_ms: Arc::new(AtomicU32::new(0)),
        packets: Arc::new(AtomicU64::new(0)),
        lost: Arc::new(AtomicU64::new(0)),
        remote_name: Arc::new(Mutex::new(String::new())),
        remote_channels: Arc::new(AtomicU32::new(0)),
        display_id: display_id.clone(),
    });

    wire_data_channel(dc, session, connection_id.clone(), node_id.to_string(), display_id.clone())
        .await;
    wire_ctrl_channel(
        ctrl_dc,
        node_id.to_string(),
        peer.ping_ms.clone(),
        peer.remote_name.clone(),
        peer.remote_channels.clone(),
        display_id.clone(),
        session.local_name.clone(),
        session.local_channels.clone(),
    )
    .await;
    session.peers.lock().await.insert(connection_id.clone(), peer);
    wire_peer_events(pc.clone(), connection_id.clone(), node_id.to_string(), session.clone(), display_id);

    Ok((connection_id, pc))
}

async fn new_guest_peer(
    node_id: &str,
    session: &Arc<WebRtcSession>,
    connection_id: &str,
    remote_sdp: String,
) -> AppResult<Arc<RTCPeerConnection>> {
    // Guest displays the host's connection_id as the remote peer label.
    let display_id = Arc::new(Mutex::new(connection_id.to_string()));

    let pc = Arc::new(new_peer_connection().await?);

    pc.on_data_channel(Box::new({
        let session = session.clone();
        let connection_id = connection_id.to_string();
        let node_id = node_id.to_string();
        let display_id = display_id.clone();
        move |dc| {
            let session = session.clone();
            let connection_id = connection_id.clone();
            let node_id = node_id.clone();
            let display_id = display_id.clone();
            Box::pin(async move {
                match dc.label().as_ref() {
                    "audio" => {
                        // Guest doesn't create the channel; store it or the encode task has no sink.
                        if let Some(peer) = session.peers.lock().await.get(&connection_id) {
                            *peer.dc.lock().unwrap() = Some(dc.clone());
                        }
                        wire_data_channel(dc, &session, connection_id, node_id, display_id).await;
                    }
                    "ctrl" => {
                        let arcs = session.peers.lock().await.get(&connection_id).map(|p| {
                            *p.ctrl_dc.lock().unwrap() = Some(dc.clone());
                            (p.ping_ms.clone(), p.remote_name.clone(), p.remote_channels.clone())
                        });
                        if let Some((ping_ms, remote_name, remote_channels)) = arcs {
                            wire_ctrl_channel(
                                dc,
                                node_id,
                                ping_ms,
                                remote_name,
                                remote_channels,
                                display_id,
                                session.local_name.clone(),
                                session.local_channels.clone(),
                            )
                            .await;
                        }
                    }
                    _ => {}
                }
            })
        }
    }));

    let peer = Arc::new(PeerState {
        peer_id: connection_id.to_string(),
        pc: pc.clone(),
        dc: Mutex::new(None),
        ctrl_dc: Mutex::new(None),
        channels: Mutex::new(HashMap::new()),
        muted: Arc::new(AtomicBool::new(false)),
        ping_ms: Arc::new(AtomicU32::new(0)),
        packets: Arc::new(AtomicU64::new(0)),
        lost: Arc::new(AtomicU64::new(0)),
        remote_name: Arc::new(Mutex::new(String::new())),
        remote_channels: Arc::new(AtomicU32::new(0)),
        display_id: display_id.clone(),
    });
    session.peers.lock().await.insert(connection_id.to_string(), peer);
    wire_peer_events(pc.clone(), connection_id.to_string(), node_id.to_string(), session.clone(), display_id);

    let offer = RTCSessionDescription::offer(remote_sdp)
        .map_err(|e| AppError::Stream(format!("parse offer SDP: {e}")))?;
    pc.set_remote_description(offer)
        .await
        .map_err(|e| AppError::Stream(format!("set remote description: {e}")))?;

    Ok(pc)
}

// Manual copy-paste exchange: all candidates gathered into the SDP, nothing to trickle.
pub async fn create_offer(
    node_id: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<(String, String)> {
    let session = get_or_create(&node_id, opus_bitrate, opus_application);
    let (connection_id, pc) = new_host_peer(&node_id, &session).await?;

    let offer = pc
        .create_offer(None)
        .await
        .map_err(|e| AppError::Stream(format!("create offer: {e}")))?;
    let done_rx = gather_done(&pc);
    pc.set_local_description(offer)
        .await
        .map_err(|e| AppError::Stream(format!("set local description: {e}")))?;
    tokio::time::timeout(Duration::from_secs(10), done_rx)
        .await
        .map_err(|_| AppError::Stream("ICE gathering timed out".into()))?
        .ok();

    let sdp = local_sdp(&pc).await?;
    info!(
        node = %node_id,
        peer = %connection_id,
        candidates = %candidate_summary(&sdp),
        "offer ready"
    );
    let offer_code = encode_sdp(&format!("{connection_id}\n{sdp}"))?;
    Ok((connection_id, offer_code))
}

// Returns the SDP at once and trickles candidates as gathered, so ICE checks live NAT mappings.
pub async fn create_offer_trickle(
    node_id: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<TrickleOffer> {
    let session = get_or_create(&node_id, opus_bitrate, opus_application);
    let (connection_id, pc) = new_host_peer(&node_id, &session).await?;

    let offer = pc
        .create_offer(None)
        .await
        .map_err(|e| AppError::Stream(format!("create offer: {e}")))?;
    let candidates = stream_candidates(&pc);
    pc.set_local_description(offer)
        .await
        .map_err(|e| AppError::Stream(format!("set local description: {e}")))?;

    let sdp = local_sdp(&pc).await?;
    info!(node = %node_id, peer = %connection_id, "trickle offer ready");
    Ok(TrickleOffer { connection_id, sdp, candidates })
}

pub async fn accept_offer(
    node_id: String,
    offer_code: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<(String, String)> {
    let payload = decode_sdp(&offer_code)?;
    let (connection_id, remote_sdp) = payload
        .split_once('\n')
        .ok_or_else(|| AppError::Stream("malformed offer code".into()))?;
    let connection_id = connection_id.to_string();
    info!(
        node = %node_id,
        peer = %connection_id,
        candidates = %candidate_summary(remote_sdp),
        "received offer"
    );

    let guest_peer_id = cuid2::create_id();
    let session = get_or_create(&node_id, opus_bitrate, opus_application);
    let pc = new_guest_peer(&node_id, &session, &connection_id, remote_sdp.to_string()).await?;

    let answer = pc
        .create_answer(None)
        .await
        .map_err(|e| AppError::Stream(format!("create answer: {e}")))?;
    let done_rx = gather_done(&pc);
    pc.set_local_description(answer)
        .await
        .map_err(|e| AppError::Stream(format!("set local description: {e}")))?;
    tokio::time::timeout(Duration::from_secs(10), done_rx)
        .await
        .map_err(|_| AppError::Stream("ICE gathering timed out".into()))?
        .ok();

    let sdp = local_sdp(&pc).await?;
    info!(
        node = %node_id,
        peer = %connection_id,
        candidates = %candidate_summary(&sdp),
        "answer ready"
    );

    // Answer carries: connection_id (map key) + guest_peer_id (shown in host UI) + sdp.
    let answer_code = encode_sdp(&format!("{connection_id}\n{guest_peer_id}\n{sdp}"))?;
    Ok((guest_peer_id, answer_code))
}

pub async fn accept_offer_trickle(
    node_id: String,
    connection_id: String,
    remote_sdp: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<TrickleAnswer> {
    info!(node = %node_id, peer = %connection_id, "received trickle offer");

    let guest_peer_id = cuid2::create_id();
    let session = get_or_create(&node_id, opus_bitrate, opus_application);
    let pc = new_guest_peer(&node_id, &session, &connection_id, remote_sdp).await?;

    let answer = pc
        .create_answer(None)
        .await
        .map_err(|e| AppError::Stream(format!("create answer: {e}")))?;
    let candidates = stream_candidates(&pc);
    pc.set_local_description(answer)
        .await
        .map_err(|e| AppError::Stream(format!("set local description: {e}")))?;

    let sdp = local_sdp(&pc).await?;
    info!(node = %node_id, peer = %connection_id, "trickle answer ready");
    Ok(TrickleAnswer { guest_peer_id, sdp, candidates, pc })
}

pub async fn complete_handshake(node_id: String, answer_code: String) -> AppResult<()> {
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
    apply_answer(&node_id, connection_id, guest_peer_id, remote_sdp).await
}

pub async fn apply_answer(
    node_id: &str,
    connection_id: &str,
    guest_peer_id: &str,
    remote_sdp: &str,
) -> AppResult<()> {
    let session = get(node_id)
        .ok_or_else(|| AppError::Validation(format!("no WebRTC session for {node_id}")))?;

    let peers = session.peers.lock().await;
    let peer = peers
        .get(connection_id)
        .ok_or_else(|| AppError::Validation(format!("no peer {connection_id} in session")))?;

    *peer.display_id.lock().unwrap() = guest_peer_id.to_string();

    let answer = RTCSessionDescription::answer(remote_sdp.to_string())
        .map_err(|e| AppError::Stream(format!("parse answer SDP: {e}")))?;
    peer.pc
        .set_remote_description(answer)
        .await
        .map_err(|e| AppError::Stream(format!("set remote description: {e}")))?;

    info!(node = %node_id, peer = %connection_id, guest = %guest_peer_id, "handshake complete");
    Ok(())
}

pub async fn add_remote_candidate(
    node_id: &str,
    connection_id: &str,
    candidate: String,
) -> AppResult<()> {
    let session = get(node_id)
        .ok_or_else(|| AppError::Validation(format!("no WebRTC session for {node_id}")))?;
    let pc = session
        .peers
        .lock()
        .await
        .get(connection_id)
        .map(|p| p.pc.clone())
        .ok_or_else(|| AppError::Validation(format!("no peer {connection_id} in session")))?;
    pc.add_ice_candidate(RTCIceCandidateInit { candidate, ..Default::default() })
        .await
        .map_err(|e| AppError::Stream(format!("add ice candidate: {e}")))
}

fn gather_done(pc: &RTCPeerConnection) -> tokio::sync::oneshot::Receiver<()> {
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
    done_rx
}

fn stream_candidates(pc: &RTCPeerConnection) -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let tx = Mutex::new(Some(tx));
    pc.on_ice_candidate(Box::new(move |candidate| {
        let mut guard = tx.lock().unwrap();
        match candidate {
            Some(c) => {
                if let (Some(tx), Ok(init)) = (guard.as_ref(), c.to_json()) {
                    let _ = tx.send(init.candidate);
                }
            }
            None => {
                guard.take();
            }
        }
        Box::pin(async {})
    }));
    rx
}

async fn local_sdp(pc: &RTCPeerConnection) -> AppResult<String> {
    Ok(pc
        .local_description()
        .await
        .ok_or_else(|| AppError::Stream("no local description".into()))?
        .sdp)
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
