use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use rtrb::{Consumer, Producer, RingBuffer};
use serde::Serialize;
use serde_json::json;
use tauri::Emitter;
use tracing::{info, warn};

use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::RTCPeerConnection;

use crate::audio::graph::OpusApplication;
use crate::audio::webrtc_codec::{decode_sdp, encode_sdp};
use crate::error::{AppError, AppResult};

// 20 ms Opus frames at 48 kHz stereo.
const OPUS_FRAME_SAMPLES: usize = 960 * 2;
const STUN_URL: &str = "stun:stun.l.google.com:19302";
const AUDIO_CHANNEL: &str = "audio";
// ~500 ms of stereo 48 kHz f32.
const RECV_RING: usize = 48_000;

pub type PeerSnapshotMap = Arc<Mutex<HashMap<String, Arc<Mutex<Vec<f32>>>>>>;

// ---------- global registry ----------

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<WebRtcSession>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<WebRtcSession>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn get_or_create(
    node_id: &str,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> Arc<WebRtcSession> {
    let mut reg = registry().lock().unwrap();
    if let Some(s) = reg.get(node_id) {
        return s.clone();
    }
    let session = Arc::new(WebRtcSession::new(node_id.to_string(), opus_bitrate, opus_application));
    reg.insert(node_id.to_string(), session.clone());
    session
}

#[allow(dead_code)]
pub fn remove(node_id: &str) {
    registry().lock().unwrap().remove(node_id);
}

pub fn mark_room(
    node_id: &str,
    opus_bitrate: u32,
    opus_application: OpusApplication,
    phase: &'static str,
    room_code: Option<String>,
) {
    let session = get_or_create(node_id, opus_bitrate, opus_application);
    *session.phase.lock().unwrap() = phase;
    *session.room_code.lock().unwrap() = room_code;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRtcPeerInfo {
    pub peer_id: String,
    pub muted: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRtcSessionState {
    /// "idle" | "hosting" | "joining".
    pub phase: String,
    pub room_code: Option<String>,
    pub peers: Vec<WebRtcPeerInfo>,
}

pub async fn session_state(node_id: &str) -> WebRtcSessionState {
    let session = match registry().lock().unwrap().get(node_id).cloned() {
        Some(s) => s,
        None => {
            return WebRtcSessionState {
                phase: "idle".to_string(),
                room_code: None,
                peers: Vec::new(),
            }
        }
    };
    let phase = session.phase.lock().unwrap().to_string();
    let room_code = session.room_code.lock().unwrap().clone();
    let peers = session
        .peers
        .lock()
        .await
        .values()
        .map(|p| WebRtcPeerInfo {
            peer_id: p.display_id.lock().unwrap().clone(),
            muted: p.muted.load(Ordering::Relaxed),
        })
        .collect();
    WebRtcSessionState {
        phase,
        room_code,
        peers,
    }
}

// ---------- session ----------

pub struct WebRtcSession {
    #[allow(dead_code)]
    pub node_id: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
    send_consumer: Mutex<Option<Consumer<f32>>>,
    peer_snapshots: PeerSnapshotMap,
    peers: tokio::sync::Mutex<HashMap<String, Arc<PeerState>>>,
    // Guard so only one encode loop runs regardless of how many peers connect.
    encoder_started: AtomicBool,
    // "idle" | "hosting" | "joining".
    phase: Mutex<&'static str>,
    room_code: Mutex<Option<String>>,
}

struct PeerState {
    peer_id: String,
    pc: Arc<RTCPeerConnection>,
    dc: Mutex<Option<Arc<RTCDataChannel>>>,
    recv_producer: Mutex<Option<Producer<f32>>>,
    decoder: Mutex<opus::Decoder>,
    recv_snapshot: Arc<Mutex<Vec<f32>>>,
    muted: Arc<AtomicBool>,
    #[allow(dead_code)]
    ping_ms: Arc<AtomicU32>,
    // The peer ID to show in the UI — the *remote* side's identity.
    // Host: starts as connection_id, updated to guestPeerId after complete_handshake.
    // Guest: set to connection_id (= host's ID) at creation.
    display_id: Arc<Mutex<String>>,
}

impl WebRtcSession {
    fn new(node_id: String, opus_bitrate: u32, opus_application: OpusApplication) -> Self {
        Self {
            node_id,
            opus_bitrate,
            opus_application,
            send_consumer: Mutex::new(None),
            peer_snapshots: Arc::new(Mutex::new(HashMap::new())),
            peers: tokio::sync::Mutex::new(HashMap::new()),
            encoder_started: AtomicBool::new(false),
            phase: Mutex::new("idle"),
            room_code: Mutex::new(None),
        }
    }

    pub fn set_send_consumer(&self, consumer: Consumer<f32>) -> PeerSnapshotMap {
        *self.send_consumer.lock().unwrap() = Some(consumer);
        self.peer_snapshots.clone()
    }
}

// ---------- offer / answer ----------

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

    let decoder = opus::Decoder::new(48000, opus::Channels::Stereo)
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
    spawn_peer_snapshot_task(cons, peer.recv_snapshot.clone());

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

    let decoder = opus::Decoder::new(48000, opus::Channels::Stereo)
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
    spawn_peer_snapshot_task(cons, peer.recv_snapshot.clone());
    session.peers.lock().await.insert(connection_id.clone(), peer);
    wire_peer_events(pc, node_id.clone(), session.clone(), display_id.clone());

    // Answer carries: connection_id (map key) + guest_peer_id (shown in host UI) + sdp.
    let answer_code = encode_sdp(&format!("{connection_id}\n{guest_peer_id}\n{sdp}"))?;
    Ok((guest_peer_id, answer_code))
}

/// Host finalises the handshake after receiving the answer code from the guest.
pub async fn complete_handshake(node_id: String, answer_code: String) -> AppResult<()> {
    let session = registry()
        .lock()
        .unwrap()
        .get(&node_id)
        .cloned()
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

pub async fn disconnect_peer(node_id: String, peer_id: String) -> AppResult<()> {
    let session = registry()
        .lock()
        .unwrap()
        .get(&node_id)
        .cloned()
        .ok_or_else(|| AppError::Validation(format!("no session for {node_id}")))?;
    session.peers.lock().await.remove(&peer_id);
    Ok(())
}

pub fn set_peer_muted(node_id: &str, peer_id: &str, muted: bool) {
    let reg = registry().lock().unwrap();
    if let Some(session) = reg.get(node_id) {
        if let Ok(peers) = session.peers.try_lock() {
            if let Some(peer) = peers.get(peer_id) {
                peer.muted.store(muted, Ordering::Relaxed);
            }
        }
    }
}

// ---------- internal helpers ----------

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

/// Returns `(display_id → ping_rtt_ms)` for all connected peers in a session.
/// Returns 0 ms for peers whose ctrl channel hasn't exchanged a ping yet.
pub fn peer_pings(node_id: &str) -> HashMap<String, u32> {
    let session_opt = registry().lock().unwrap().get(node_id).cloned();
    if session_opt.is_none() {
        return HashMap::new();
    }
    let session = session_opt.unwrap();
    let result: HashMap<String, u32> = if let Ok(peers) = session.peers.try_lock() {
        peers.values().map(|p| {
            let id = p.display_id.lock().unwrap().clone();
            let ms = p.ping_ms.load(Ordering::Relaxed);
            (id, ms)
        }).collect()
    } else {
        HashMap::new()
    };
    result
}

// Ctrl DataChannel: "P{ts_ms}" = ping, "Q{ts_ms}" = pong.
// Both sides ping each other independently; each updates its own ping_ms.
async fn wire_ctrl_channel(dc: Arc<RTCDataChannel>, ping_ms: Arc<AtomicU32>) {
    let dc_open = dc.clone();
    let dc_msg = dc.clone();

    dc.on_open(Box::new(move || {
        let dc = dc_open.clone();
        Box::pin(async move {
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let _ = dc.send_text(format!("P{ts}")).await;
                }
            });
        })
    }));

    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let dc = dc_msg.clone();
        let ping_ms = ping_ms.clone();
        Box::pin(async move {
            if !msg.is_string { return; }
            let Ok(text) = String::from_utf8(msg.data.to_vec()) else { return };
            if let Some(ts_str) = text.strip_prefix('P') {
                let _ = dc.send_text(format!("Q{ts_str}")).await;
            } else if let Some(ts_str) = text.strip_prefix('Q') {
                if let Ok(ts) = ts_str.parse::<u64>() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    ping_ms.store(now.saturating_sub(ts) as u32, Ordering::Relaxed);
                }
            }
        })
    }));
}

async fn wire_data_channel(
    dc: Arc<RTCDataChannel>,
    session: &Arc<WebRtcSession>,
    peer_id: String,
    node_id: String,
    display_id: Arc<Mutex<String>>,
) {
    let session_send = session.clone();
    let session_recv = session.clone();
    let peer_id_recv = peer_id.clone();

    dc.on_open(Box::new({
        let session = session_send.clone();
        let node_id = node_id.clone();
        let display_id = display_id.clone();
        let peer_id = peer_id.clone();
        move || {
            let session = session.clone();
            let node_id = node_id.clone();
            let display_id = display_id.clone();
            let peer_id = peer_id.clone();
            Box::pin(async move {
                // Only one encode loop per session regardless of peer count.
                if !session.encoder_started.swap(true, Ordering::SeqCst) {
                    spawn_encode_task(session.clone());
                }
                let remote_id = display_id.lock().unwrap().clone();
                let snapshot = {
                    let peers = session.peers.lock().await;
                    peers.get(&peer_id).map(|p| p.recv_snapshot.clone())
                };
                if let Some(snapshot) = snapshot {
                    session
                        .peer_snapshots
                        .lock()
                        .unwrap()
                        .insert(remote_id.clone(), snapshot);
                }
                if let Some(app) = crate::app_handle() {
                    let _ = app.emit(
                        "audio://webrtc_connected",
                        json!({ "nodeId": node_id, "peerId": remote_id }),
                    );
                }
            })
        }
    }));

    dc.on_message(Box::new(
        move |msg: webrtc::data_channel::data_channel_message::DataChannelMessage| {
            let session = session_recv.clone();
            let peer_id = peer_id_recv.clone();
            Box::pin(async move { decode_and_write(msg.data, &session, &peer_id).await; })
        },
    ));
}

fn wire_peer_events(
    pc: Arc<RTCPeerConnection>,
    node_id: String,
    session: Arc<WebRtcSession>,
    display_id: Arc<Mutex<String>>,
) {
    use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
    pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        let node_id = node_id.clone();
        let session = session.clone();
        let display_id = display_id.clone();
        Box::pin(async move {
            // Connected is emitted from DataChannel on_open instead, so we
            // only handle terminal states here.
            let event = match state {
                RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Closed => "audio://webrtc_disconnected",
                _ => return,
            };
            let remote_id = display_id.lock().unwrap().clone();
            session.peer_snapshots.lock().unwrap().remove(&remote_id);
            info!(node = %node_id, peer = %remote_id, ?state, "peer state changed");
            if let Some(app) = crate::app_handle() {
                let _ = app.emit(event, json!({ "nodeId": node_id, "peerId": remote_id }));
            }
        })
    }));
}

fn spawn_encode_task(session: Arc<WebRtcSession>) {
    let bitrate = session.opus_bitrate;
    let application = match session.opus_application {
        OpusApplication::Voip => opus::Application::Voip,
        OpusApplication::Audio => opus::Application::Audio,
        OpusApplication::LowDelay => opus::Application::LowDelay,
    };

    tauri::async_runtime::spawn(async move {
        let mut encoder = match opus::Encoder::new(48000, opus::Channels::Stereo, application) {
            Ok(e) => e,
            Err(e) => { warn!(error = %e, "opus encoder init failed"); return; }
        };
        if let Err(e) = encoder.set_bitrate(opus::Bitrate::Bits(bitrate as i32)) {
            warn!(error = %e, "set opus bitrate failed");
        }

        let mut pcm = vec![0.0_f32; OPUS_FRAME_SAMPLES];
        let mut opus_buf = vec![0u8; 4096];
        let mut interval = tokio::time::interval(Duration::from_millis(20));

        loop {
            interval.tick().await;

            {
                let mut cons_guard = session.send_consumer.lock().unwrap();
                if let Some(cons) = cons_guard.as_mut() {
                    let take = cons.slots().min(OPUS_FRAME_SAMPLES);
                    if take > 0 {
                        if let Ok(chunk) = cons.read_chunk(take) {
                            let (a, b) = chunk.as_slices();
                            pcm[..a.len()].copy_from_slice(a);
                            if !b.is_empty() {
                                pcm[a.len()..a.len() + b.len()].copy_from_slice(b);
                            }
                            if take < OPUS_FRAME_SAMPLES {
                                pcm[take..].fill(0.0);
                            }
                            chunk.commit_all();
                        }
                    } else {
                        pcm.fill(0.0);
                    }
                } else {
                    pcm.fill(0.0);
                }
            }

            match encoder.encode_float(&pcm, &mut opus_buf) {
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
    });
}

async fn decode_and_write(data: Bytes, session: &Arc<WebRtcSession>, peer_id: &str) {
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

fn spawn_peer_snapshot_task(mut consumer: Consumer<f32>, recv_snapshot: Arc<Mutex<Vec<f32>>>) {
    tauri::async_runtime::spawn(async move {
        let mut buf = vec![0.0_f32; OPUS_FRAME_SAMPLES];
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        loop {
            interval.tick().await;
            let avail = consumer.slots().min(OPUS_FRAME_SAMPLES);
            if avail > 0 {
                if let Ok(chunk) = consumer.read_chunk(avail) {
                    let (a, b) = chunk.as_slices();
                    buf[..a.len()].copy_from_slice(a);
                    if !b.is_empty() {
                        buf[a.len()..a.len() + b.len()].copy_from_slice(b);
                    }
                    if avail < OPUS_FRAME_SAMPLES {
                        buf[avail..].fill(0.0);
                    }
                    chunk.commit_all();
                    if let Ok(mut snap) = recv_snapshot.try_lock() {
                        let n = buf.len().min(snap.len());
                        snap[..n].copy_from_slice(&buf[..n]);
                    }
                }
            }
        }
    });
}
