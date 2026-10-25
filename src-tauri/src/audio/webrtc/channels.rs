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

#[derive(Debug, PartialEq)]
enum CtrlMsg<'a> {
    Bye,
    Ping(&'a str),
    Pong(u64),
    Meta { name: String, channels: u32 },
}

fn parse_ctrl(text: &str) -> Option<CtrlMsg<'_>> {
    if text == "B" {
        return Some(CtrlMsg::Bye);
    }
    if let Some(ts) = text.strip_prefix('P') {
        return Some(CtrlMsg::Ping(ts));
    }
    if let Some(ts) = text.strip_prefix('Q') {
        return ts.parse().ok().map(CtrlMsg::Pong);
    }
    let meta = text.strip_prefix('M')?;
    let v = serde_json::from_str::<serde_json::Value>(meta).ok()?;
    Some(CtrlMsg::Meta {
        name: v
            .get("n")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        channels: v.get("c").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
    })
}

// Ctrl DataChannel messages: "P{ts}" ping, "Q{ts}" pong, "M{json}" identity
// ({"n":name,"c":inputCount}) resent every tick so late listeners and renames
// converge without a one-shot race.
#[allow(clippy::too_many_arguments)]
pub async fn wire_ctrl_channel(
    dc: Arc<RTCDataChannel>,
    node_id: String,
    ping_ms: Arc<AtomicU32>,
    remote_name: Arc<Mutex<String>>,
    remote_channels: Arc<AtomicU32>,
    display_id: Arc<Mutex<String>>,
    local_name: Arc<Mutex<String>>,
    local_channels: Arc<AtomicU32>,
) {
    let dc_open = dc.clone();
    let dc_msg = dc.clone();

    dc.on_open(Box::new(move || {
        let dc = dc_open.clone();
        let local_name = local_name.clone();
        let local_channels = local_channels.clone();
        Box::pin(async move {
            tauri::async_runtime::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    let name = local_name.lock().unwrap().clone();
                    let chans = local_channels.load(Ordering::Relaxed);
                    let meta = json!({ "n": name, "c": chans }).to_string();
                    let _ = dc.send_text(format!("M{meta}")).await;
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
        let remote_name = remote_name.clone();
        let remote_channels = remote_channels.clone();
        let display_id = display_id.clone();
        let node_id = node_id.clone();
        Box::pin(async move {
            if !msg.is_string { return; }
            let Ok(text) = String::from_utf8(msg.data.to_vec()) else { return };
            match parse_ctrl(&text) {
                Some(CtrlMsg::Bye) => {
                    // Peer is leaving; surface it now rather than on ICE timeout.
                    let peer = display_id.lock().unwrap().clone();
                    if let Some(app) = crate::app_handle() {
                        let _ = app.emit(
                            "audio://webrtc_disconnected",
                            json!({ "nodeId": node_id, "peerId": peer }),
                        );
                    }
                }
                Some(CtrlMsg::Ping(ts)) => {
                    let _ = dc.send_text(format!("Q{ts}")).await;
                }
                Some(CtrlMsg::Pong(ts)) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    ping_ms.store(now.saturating_sub(ts) as u32, Ordering::Relaxed);
                }
                Some(CtrlMsg::Meta { name, channels: chans }) => {
                    let changed = {
                        let mut cur = remote_name.lock().unwrap();
                        let name_changed = *cur != name;
                        *cur = name.clone();
                        name_changed || remote_channels.swap(chans, Ordering::Relaxed) != chans
                    };
                    if !changed { return; }
                    let peer = display_id.lock().unwrap().clone();
                    if let Some(app) = crate::app_handle() {
                        let _ = app.emit(
                            "audio://webrtc_meta",
                            json!({ "nodeId": node_id, "peerId": peer, "name": name, "channels": chans }),
                        );
                    }
                }
                None => {}
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
        move || {
            let session = session.clone();
            let node_id = node_id.clone();
            let display_id = display_id.clone();
            Box::pin(async move {
                // Only one encode loop per session regardless of peer count.
                if !session.encoder_started.swap(true, Ordering::SeqCst) {
                    spawn_encode_task(session.clone());
                }
                let remote_id = display_id.lock().unwrap().clone();
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
        Box::pin(async move {
            decode_and_write(msg.data, &session, &peer_id).await;
        })
    }));
}

/// Returns every state the connection goes through, for whoever waits on it
/// to connect: a connection takes one state handler, and this is it.
pub fn wire_peer_events(
    pc: Arc<RTCPeerConnection>,
    connection_id: String,
    node_id: String,
    session: Arc<WebRtcSession>,
    display_id: Arc<Mutex<String>>,
) -> tokio::sync::watch::Receiver<
    webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState,
> {
    use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
    let (state_tx, state_rx) = tokio::sync::watch::channel(RTCPeerConnectionState::New);
    let self_pc = pc.clone();
    pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
        let _ = state_tx.send(state);
        let connection_id = connection_id.clone();
        let node_id = node_id.clone();
        let session = session.clone();
        let display_id = display_id.clone();
        let self_pc = self_pc.clone();
        Box::pin(async move {
            // Only Failed/Closed are terminal; Disconnected is often transient
            // and can recover to Connected, so we don't tear the peer down on it.
            let event = match state {
                RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                    "audio://webrtc_disconnected"
                }
                _ => return,
            };
            let remote_id = display_id.lock().unwrap().clone();
            // Drop the peer. Only if the map still holds this pc: a retry may
            // have reused the connection_id with a new one.
            {
                let mut peers = session.peers.lock().await;
                let is_current = peers
                    .get(&connection_id)
                    .map(|p| Arc::ptr_eq(&p.pc, &self_pc))
                    .unwrap_or(false);
                if is_current {
                    peers.remove(&connection_id);
                }
            }
            session.drop_peer_channels(&remote_id);
            info!(node = %node_id, peer = %remote_id, ?state, "peer state changed");
            if let Some(app) = crate::app_handle() {
                let _ = app.emit(event, json!({ "nodeId": node_id, "peerId": remote_id }));
            }
        })
    }));
    state_rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_ctrl_message() {
        assert_eq!(parse_ctrl("B"), Some(CtrlMsg::Bye));
        assert_eq!(parse_ctrl("P123"), Some(CtrlMsg::Ping("123")));
        assert_eq!(parse_ctrl("Q456"), Some(CtrlMsg::Pong(456)));
        assert_eq!(
            parse_ctrl(r#"M{"n":"Studio","c":4}"#),
            Some(CtrlMsg::Meta {
                name: "Studio".into(),
                channels: 4
            })
        );
    }

    #[test]
    fn meta_fields_default_when_missing() {
        assert_eq!(
            parse_ctrl("M{}"),
            Some(CtrlMsg::Meta {
                name: String::new(),
                channels: 0
            })
        );
    }

    #[test]
    fn malformed_ctrl_messages_are_ignored() {
        assert_eq!(parse_ctrl(""), None);
        assert_eq!(parse_ctrl("Bye"), None);
        assert_eq!(parse_ctrl("Qabc"), None, "pong needs a numeric timestamp");
        assert_eq!(parse_ctrl("Q-1"), None);
        assert_eq!(parse_ctrl("M{not json"), None);
        assert_eq!(parse_ctrl("X1"), None);
    }
}
