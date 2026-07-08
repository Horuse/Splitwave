use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tauri::Emitter;
use tracing::info;

use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::RTCPeerConnection;

use super::session::WebRtcSession;
use super::tasks::{decode_and_write, spawn_encode_task};

// Ctrl DataChannel: "P{ts_ms}" = ping, "Q{ts_ms}" = pong.
// Both sides ping each other independently; each updates its own ping_ms.
pub async fn wire_ctrl_channel(dc: Arc<RTCDataChannel>, ping_ms: Arc<AtomicU32>) {
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
                    // A send error means the channel is closed -- stop pinging.
                    if dc.send_text(format!("P{ts}")).await.is_err() {
                        break;
                    }
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

pub async fn wire_data_channel(
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

    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let session = session_recv.clone();
        let peer_id = peer_id_recv.clone();
        Box::pin(async move { decode_and_write(msg.data, &session, &peer_id).await; })
    }));
}

pub fn wire_peer_events(
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
