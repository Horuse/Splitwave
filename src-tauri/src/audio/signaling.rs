use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::error::{AppError, AppResult};

const SIG_BASE: &str = "wss://sig.splitwave.app";

#[derive(Serialize)]
struct OutMsg<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    #[serde(rename = "peerId", skip_serializing_if = "Option::is_none")]
    peer_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp: Option<&'a str>,
}

#[derive(Deserialize)]
struct InMsg {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "peerId")]
    peer_id: Option<String>,
    sdp: Option<String>,
}

fn sig_err(e: impl std::fmt::Display) -> AppError {
    AppError::Stream(format!("signaling: {e}"))
}

/// Host: sends offer, blocks until guest's answer arrives.
/// Returns `(guestPeerId, answerSdp)`.
pub async fn host_exchange(
    room_code: &str,
    host_peer_id: &str,
    offer_sdp: &str,
) -> AppResult<(String, String)> {
    let url = format!("{SIG_BASE}/ws/{room_code}?role=host&peerId={host_peer_id}");
    let (mut ws, _) = connect_async(url).await.map_err(sig_err)?;

    let payload = serde_json::to_string(&OutMsg {
        kind: "offer",
        peer_id: Some(host_peer_id),
        sdp: Some(offer_sdp),
    })
    .unwrap();
    ws.send(Message::text(payload)).await.map_err(sig_err)?;

    while let Some(frame) = ws.next().await {
        let frame = frame.map_err(sig_err)?;
        if let Ok(text) = frame.into_text() {
            if let Ok(m) = serde_json::from_str::<InMsg>(&text) {
                if m.kind == "answer" {
                    if let (Some(pid), Some(sdp)) = (m.peer_id, m.sdp) {
                        let _ = ws.close(None).await;
                        return Ok((pid, sdp));
                    }
                }
            }
        }
    }
    Err(sig_err("connection closed before answer"))
}

/// Guest: connects, receives host's offer, then calls `accept_fn(hostPeerId, offerSdp)`
/// and sends the returned `(guestPeerId, answerSdp)` back to the host.
pub async fn guest_exchange<F, Fut>(room_code: &str, accept_fn: F) -> AppResult<()>
where
    F: FnOnce(String, String) -> Fut,
    Fut: std::future::Future<Output = AppResult<(String, String)>>,
{
    let url = format!("{SIG_BASE}/ws/{room_code}?role=guest");
    let (mut ws, _) = connect_async(url).await.map_err(sig_err)?;

    let (host_peer_id, offer_sdp) = loop {
        match ws.next().await {
            Some(Ok(frame)) => {
                if let Ok(text) = frame.into_text() {
                    if let Ok(m) = serde_json::from_str::<InMsg>(&text) {
                        if m.kind == "offer" {
                            if let (Some(pid), Some(sdp)) = (m.peer_id, m.sdp) {
                                break (pid, sdp);
                            }
                        }
                    }
                }
            }
            Some(Err(e)) => return Err(sig_err(e)),
            None => return Err(sig_err("connection closed before offer")),
        }
    };

    let (guest_peer_id, answer_sdp) = accept_fn(host_peer_id, offer_sdp).await?;

    let payload = serde_json::to_string(&OutMsg {
        kind: "answer",
        peer_id: Some(&guest_peer_id),
        sdp: Some(&answer_sdp),
    })
    .unwrap();
    ws.send(Message::text(payload)).await.map_err(sig_err)?;
    let _ = ws.close(None).await;
    Ok(())
}

/// Generates a random 6-character room code (uppercase, no ambiguous chars).
pub fn random_room_code() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
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
