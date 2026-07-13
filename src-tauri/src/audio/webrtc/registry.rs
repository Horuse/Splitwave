use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;

use crate::audio::graph::{NetCodec, OpusApplication};

use super::session::{PeerState, WebRtcSession};

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<WebRtcSession>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<WebRtcSession>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn get(node_id: &str) -> Option<Arc<WebRtcSession>> {
    registry().lock().unwrap().get(node_id).cloned()
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

pub fn set_signaling_task(node_id: &str, task: tokio::task::JoinHandle<()>) {
    if let Some(session) = registry().lock().unwrap().get(node_id) {
        *session.signaling_task.lock().unwrap() = Some(task);
    }
}

/// Cancels a room: aborts the in-flight signaling exchange, drops every peer
/// and resets phase/code. The session itself stays in the registry so the
/// running pipeline keeps its bridge wiring and the node can host/join again.
pub async fn leave_room(node_id: &str) {
    let session = match get(node_id) {
        Some(s) => s,
        None => return,
    };
    let signaling_task = session.signaling_task.lock().unwrap().take();
    if let Some(task) = signaling_task {
        task.abort();
        let _ = task.await;
    }
    *session.phase.lock().unwrap() = "idle";
    *session.room_code.lock().unwrap() = None;
    let peers: Vec<Arc<PeerState>> =
        session.peers.lock().await.drain().map(|(_, p)| p).collect();
    for peer in &peers {
        // Tell peers we're leaving so they react immediately instead of waiting
        // out the ICE disconnect timeout.
        let ctrl = peer.ctrl_dc.lock().unwrap().clone();
        if let Some(dc) = ctrl {
            let _ = dc.send_text("B".to_string()).await;
        }
    }
    for peer in peers {
        let _ = peer.pc.close().await;
    }
    session.fanout.clear();
}

/// Stores the local name and input count; the ctrl channel's periodic meta
/// message carries them to peers.
pub fn set_identity(node_id: &str, name: String, channels: u32, codec: NetCodec) {
    if let Some(session) = get(node_id) {
        *session.local_name.lock().unwrap() = name;
        session.local_channels.store(channels.clamp(1, 10), Ordering::Relaxed);
        session.set_codec(codec);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebRtcPeerInfo {
    pub peer_id: String,
    pub muted: bool,
    pub name: String,
    /// This peer's declared input indices (0..count).
    pub channels: Vec<u8>,
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
    let session = match get(node_id) {
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
        .map(|p| {
            let count = p.remote_channels.load(Ordering::Relaxed);
            WebRtcPeerInfo {
                peer_id: p.display_id.lock().unwrap().clone(),
                muted: p.muted.load(Ordering::Relaxed),
                name: p.remote_name.lock().unwrap().clone(),
                channels: (0..count as u8).collect(),
            }
        })
        .collect();
    WebRtcSessionState {
        phase,
        room_code,
        peers,
    }
}

pub async fn disconnect_peer(node_id: String, peer_id: String) -> crate::error::AppResult<()> {
    let session = get(&node_id)
        .ok_or_else(|| crate::error::AppError::Validation(format!("no session for {node_id}")))?;
    session.peers.lock().await.remove(&peer_id);
    Ok(())
}

pub fn set_peer_muted(node_id: &str, peer_id: &str, muted: bool) {
    if let Some(session) = get(node_id) {
        if let Ok(peers) = session.peers.try_lock() {
            if let Some(peer) = peers.get(peer_id) {
                peer.muted.store(muted, Ordering::Relaxed);
            }
        }
    }
}

/// Returns `(display_id → ping_rtt_ms)` for all connected peers in a session.
/// Returns 0 ms for peers whose ctrl channel hasn't exchanged a ping yet.
pub fn peer_pings(node_id: &str) -> HashMap<String, u32> {
    let Some(session) = get(node_id) else {
        return HashMap::new();
    };
    let result = if let Ok(peers) = session.peers.try_lock() {
        peers
            .values()
            .map(|p| {
                let id = p.display_id.lock().unwrap().clone();
                let ms = p.ping_ms.load(Ordering::Relaxed);
                (id, ms)
            })
            .collect()
    } else {
        HashMap::new()
    };
    result
}

/// Returns `(display_id -> (ping_ms, loss_ratio))` for connected peers. Loss is
/// the fraction of inferred-missing packets over received-plus-missing.
pub fn peer_stats(node_id: &str) -> HashMap<String, (u32, f32)> {
    let Some(session) = get(node_id) else {
        return HashMap::new();
    };
    let Ok(peers) = session.peers.try_lock() else {
        return HashMap::new();
    };
    peers
        .values()
        .map(|p| {
            let id = p.display_id.lock().unwrap().clone();
            let ms = p.ping_ms.load(Ordering::Relaxed);
            let recv = p.packets.load(Ordering::Relaxed);
            let lost = p.lost.load(Ordering::Relaxed);
            let total = recv + lost;
            let loss = if total == 0 { 0.0 } else { lost as f32 / total as f32 };
            (id, (ms, loss))
        })
        .collect()
}
