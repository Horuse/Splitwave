use std::collections::HashMap;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tracing::{info, warn};

use crate::audio::graph::OpusApplication;
use crate::audio::webrtc;
use crate::error::{AppError, AppResult};

const SIG_BASE: &str = "wss://sig.splitwave.app";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const JOIN_ATTEMPTS: u32 = 3;

type Ws = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Serialize)]
struct OutMsg<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    #[serde(rename = "joinId", skip_serializing_if = "Option::is_none")]
    join_id: Option<&'a str>,
    #[serde(rename = "peerId", skip_serializing_if = "Option::is_none")]
    peer_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate: Option<&'a str>,
}

#[derive(Deserialize)]
struct InMsg {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "joinId")]
    join_id: Option<String>,
    #[serde(rename = "peerId")]
    peer_id: Option<String>,
    sdp: Option<String>,
    candidate: Option<String>,
    reason: Option<String>,
}

fn sig_err(e: impl std::fmt::Display) -> AppError {
    AppError::Stream(format!("signaling: {e}"))
}

async fn send_msg(ws: &mut Ws, msg: &OutMsg<'_>) -> AppResult<()> {
    ws.send(Message::text(serde_json::to_string(msg).unwrap()))
        .await
        .map_err(sig_err)
}

fn parse_msg(frame: Message) -> Option<InMsg> {
    let text = frame.into_text().ok()?;
    serde_json::from_str::<InMsg>(&text).ok()
}

// Fresh trickle offer per guest: pre-gathered offers go stale as NAT mappings expire.
pub async fn host_loop(
    room_code: String,
    password_hash: String,
    node_id: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<()> {
    let url = format!("{SIG_BASE}/ws/{room_code}?role=host&passwordHash={password_hash}");
    loop {
        let (ws, _) = connect_async(&url).await.map_err(sig_err)?;
        host_session(ws, &node_id, opus_bitrate, opus_application).await;
        info!(room = %room_code, "signaling reconnect");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn host_session(
    mut ws: Ws,
    node_id: &str,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) {
    // joinId -> connection_id of that guest's offer.
    let mut joins: HashMap<String, String> = HashMap::new();
    let (cand_tx, mut cand_rx) = tokio::sync::mpsc::unbounded_channel::<(String, String)>();
    let mut keepalive = tokio::time::interval(Duration::from_secs(30));
    keepalive.tick().await;

    loop {
        tokio::select! {
            frame = ws.next() => {
                let Some(Ok(frame)) = frame else { return };
                let Some(m) = parse_msg(frame) else { continue };
                match m.kind.as_str() {
                    "join" => {
                        let Some(join_id) = m.join_id else { continue };
                        let offer = match webrtc::create_offer_trickle(
                            node_id.to_string(), opus_bitrate, opus_application,
                        ).await {
                            Ok(o) => o,
                            Err(e) => { warn!(error = %e, "create offer for join"); continue }
                        };
                        joins.insert(join_id.clone(), offer.connection_id.clone());
                        let msg = OutMsg {
                            kind: "offer",
                            join_id: Some(&join_id),
                            peer_id: Some(&offer.connection_id),
                            sdp: Some(&offer.sdp),
                            candidate: None,
                        };
                        if send_msg(&mut ws, &msg).await.is_err() { return }
                        let tx = cand_tx.clone();
                        let mut rx = offer.candidates;
                        tokio::spawn(async move {
                            while let Some(c) = rx.recv().await {
                                if tx.send((join_id.clone(), c)).is_err() { break }
                            }
                        });
                    }
                    "answer" => {
                        let (Some(join_id), Some(guest_peer_id), Some(sdp)) =
                            (m.join_id, m.peer_id, m.sdp) else { continue };
                        let Some(connection_id) = joins.get(&join_id) else { continue };
                        if let Err(e) =
                            webrtc::apply_answer(node_id, connection_id, &guest_peer_id, &sdp).await
                        {
                            warn!(error = %e, "apply answer");
                        }
                    }
                    "candidate" => {
                        let (Some(join_id), Some(candidate)) = (m.join_id, m.candidate)
                            else { continue };
                        let Some(connection_id) = joins.get(&join_id) else { continue };
                        if let Err(e) =
                            webrtc::add_remote_candidate(node_id, connection_id, candidate).await
                        {
                            warn!(error = %e, "add remote candidate");
                        }
                    }
                    _ => {}
                }
            }
            Some((join_id, candidate)) = cand_rx.recv() => {
                let msg = OutMsg {
                    kind: "candidate",
                    join_id: Some(&join_id),
                    peer_id: None,
                    sdp: None,
                    candidate: Some(&candidate),
                };
                if send_msg(&mut ws, &msg).await.is_err() { return }
            }
            _ = keepalive.tick() => {
                if ws.send(Message::Ping(Vec::new().into())).await.is_err() { return }
            }
        }
    }
}

// ICE failure retries with a fresh join, which makes the host mint a fresh offer.
pub async fn guest_join(
    room_code: String,
    password_hash: String,
    node_id: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<()> {
    for attempt in 1..=JOIN_ATTEMPTS {
        match guest_attempt(&room_code, &password_hash, &node_id, opus_bitrate, opus_application)
            .await
        {
            Ok(true) => return Ok(()),
            Ok(false) => info!(attempt, "join attempt failed, retrying"),
            Err(e) => return Err(e),
        }
    }
    Err(AppError::Stream(format!(
        "could not connect after {JOIN_ATTEMPTS} attempts"
    )))
}

/// Ok(true) = connected, Ok(false) = retryable failure, Err = fatal.
async fn guest_attempt(
    room_code: &str,
    password_hash: &str,
    node_id: &str,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<bool> {
    let url = format!("{SIG_BASE}/ws/{room_code}?role=guest&passwordHash={password_hash}");
    let (mut ws, _) = connect_async(&url).await.map_err(sig_err)?;

    let (connection_id, offer_sdp) = loop {
        match ws.next().await {
            Some(Ok(frame)) => {
                let Some(m) = parse_msg(frame) else { continue };
                match m.kind.as_str() {
                    "error" => {
                        return Err(sig_err(match m.reason.as_deref() {
                            Some("password") => "wrong password",
                            Some("no-host") => "room not found",
                            _ => "rejected by room",
                        }))
                    }
                    "offer" => {
                        if let (Some(pid), Some(sdp)) = (m.peer_id, m.sdp) {
                            break (pid, sdp);
                        }
                    }
                    _ => {}
                }
            }
            Some(Err(e)) => return Err(sig_err(e)),
            None => return Ok(false),
        }
    };

    let answer = webrtc::accept_offer_trickle(
        node_id.to_string(),
        connection_id.clone(),
        offer_sdp,
        opus_bitrate,
        opus_application,
    )
    .await?;

    let msg = OutMsg {
        kind: "answer",
        join_id: None,
        peer_id: Some(&answer.guest_peer_id),
        sdp: Some(&answer.sdp),
        candidate: None,
    };
    send_msg(&mut ws, &msg).await?;

    let mut candidates = answer.candidates;
    let pc = answer.pc;
    let mut cands_done = false;
    let mut ws_done = false;
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    let mut tick = tokio::time::interval(Duration::from_millis(250));

    loop {
        tokio::select! {
            _ = tick.tick() => {
                use ::webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState as S;
                match pc.connection_state() {
                    S::Connected => {
                        let _ = ws.close(None).await;
                        return Ok(true);
                    }
                    S::Failed | S::Closed => return Ok(false),
                    _ if tokio::time::Instant::now() >= deadline => {
                        let _ = pc.close().await;
                        return Ok(false);
                    }
                    _ => {}
                }
            }
            c = candidates.recv(), if !cands_done => {
                match c {
                    Some(c) => {
                        let msg = OutMsg {
                            kind: "candidate",
                            join_id: None,
                            peer_id: None,
                            sdp: None,
                            candidate: Some(&c),
                        };
                        if send_msg(&mut ws, &msg).await.is_err() { ws_done = true }
                    }
                    None => cands_done = true,
                }
            }
            frame = ws.next(), if !ws_done => {
                match frame {
                    Some(Ok(frame)) => {
                        let Some(m) = parse_msg(frame) else { continue };
                        if m.kind == "candidate" {
                            if let Some(c) = m.candidate {
                                if let Err(e) =
                                    webrtc::add_remote_candidate(node_id, &connection_id, c).await
                                {
                                    warn!(error = %e, "add remote candidate");
                                }
                            }
                        }
                    }
                    _ => ws_done = true,
                }
            }
        }
    }
}

pub fn random_room_code() -> String {
    const CHARS: &[u8] = b"0123456789";
    use std::time::SystemTime;
    let mut n = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    for b in format!("{:?}", std::thread::current().id()).bytes() {
        n = n.wrapping_mul(31).wrapping_add(b as u32);
    }
    (0..6)
        .map(|_| {
            let idx = (n % CHARS.len() as u32) as usize;
            n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            CHARS[idx] as char
        })
        .collect()
}
