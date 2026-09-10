use std::collections::HashMap;

use tauri::{AppHandle, Emitter};

use crate::audio::graph::OpusApplication;
use crate::audio::{signaling, webrtc};
use crate::error::AppResult;

#[tauri::command]
pub async fn webrtc_create_offer(
    node_id: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<serde_json::Value> {
    let (peer_id, offer_code) =
        webrtc::create_offer(node_id, opus_bitrate, opus_application).await?;
    Ok(serde_json::json!({ "peerId": peer_id, "offerCode": offer_code }))
}

#[tauri::command]
pub async fn webrtc_accept_offer(
    node_id: String,
    offer_code: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) -> AppResult<serde_json::Value> {
    let (peer_id, answer_code) =
        webrtc::accept_offer(node_id, offer_code, opus_bitrate, opus_application).await?;
    Ok(serde_json::json!({ "peerId": peer_id, "answerCode": answer_code }))
}

#[tauri::command]
pub async fn webrtc_complete_handshake(node_id: String, answer_code: String) -> AppResult<()> {
    webrtc::complete_handshake(node_id, answer_code).await
}

#[tauri::command]
pub async fn webrtc_disconnect_peer(node_id: String, peer_id: String) -> AppResult<()> {
    webrtc::disconnect_peer(node_id, peer_id).await
}

#[tauri::command]
pub fn webrtc_set_peer_muted(node_id: String, peer_id: String, muted: bool) {
    webrtc::set_peer_muted(&node_id, &peer_id, muted);
}

#[tauri::command]
pub fn webrtc_peer_pings(node_id: String) -> HashMap<String, u32> {
    webrtc::peer_pings(&node_id)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerStats {
    pub ping_ms: u32,
    pub packets: u64,
    pub lost: u64,
}

/// Per-peer receive quality: RTT ping plus cumulative received/lost packet
/// counters (windowed into a recent loss ratio on the frontend).
#[tauri::command]
pub fn webrtc_peer_stats(node_id: String) -> HashMap<String, PeerStats> {
    webrtc::peer_stats(&node_id)
        .into_iter()
        .map(|(id, (ping_ms, packets, lost))| {
            (
                id,
                PeerStats {
                    ping_ms,
                    packets,
                    lost,
                },
            )
        })
        .collect()
}

/// Jitter buffer depth in ms, the latency this node adds. Session-wide: unlike
/// ping, it is a property of the buffer every peer plays out of.
#[tauri::command]
pub fn webrtc_buffer_ms(node_id: String) -> u32 {
    webrtc::buffer_ms(&node_id)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetReceiverStats {
    pub bytes: u64,
    pub packets: u64,
    pub lost: u64,
    pub channels: u32,
    pub buffer_ms: u32,
    pub sample_rate: u32,
    pub format: Option<String>,
    pub opus_bitrate: Option<u32>,
    pub opus_app: Option<String>,
}

/// The DAG only binds receivers reachable from an output, so an unrouted node
/// could never report the stream it would carry.
#[tauri::command]
pub fn net_receiver_listen(node_id: String, port: u16) {
    crate::audio::netaudio::receiver::get_or_create(&node_id, port);
}

#[tauri::command]
pub fn net_receiver_release(node_id: String) {
    crate::audio::netaudio::receiver::release(&node_id);
}

/// Direct-IP receive stats: cumulative bytes, packets, and lost packets
/// (windowed into a recent loss ratio / rate on the frontend).
#[tauri::command]
pub fn net_receiver_stats(node_id: String) -> Option<NetReceiverStats> {
    crate::audio::netaudio::receiver::stats(&node_id).map(|s| {
        let format_str = s.format.map(|f| match f {
            crate::audio::netaudio::packet::Format::PcmF32 => "pcm-f32".to_string(),
            crate::audio::netaudio::packet::Format::PcmI16 => "pcm-i16".to_string(),
            crate::audio::netaudio::packet::Format::Opus => "opus".to_string(),
        });
        let opus_app_str = s.opus_app.and_then(|a| match a {
            1 => Some("voip".to_string()),
            2 => Some("audio".to_string()),
            3 => Some("low-delay".to_string()),
            _ => None,
        });
        NetReceiverStats {
            bytes: s.bytes,
            packets: s.packets,
            lost: s.lost,
            channels: s.channels,
            buffer_ms: s.buffer_ms,
            sample_rate: s.sample_rate,
            format: format_str,
            opus_bitrate: s.opus_bitrate,
            opus_app: opus_app_str,
        }
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetSenderStats {
    pub bytes: u64,
    pub packets: u64,
}

/// Direct-IP send stats: total bytes and packets transmitted.
#[tauri::command]
pub fn net_sender_stats(node_id: String) -> Option<NetSenderStats> {
    crate::audio::netaudio::sender::stats(&node_id)
        .map(|(bytes, packets)| NetSenderStats { bytes, packets })
}

/// Stores the local participant name and input count; peers receive them via
/// the ctrl channel's periodic meta message.
#[tauri::command]
pub fn webrtc_set_identity(
    node_id: String,
    name: String,
    channels: u32,
    codec: crate::audio::graph::NetCodec,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) {
    webrtc::get_or_create(&node_id, opus_bitrate, opus_application);
    webrtc::set_identity(&node_id, name, channels, codec);
}

#[tauri::command]
pub async fn webrtc_session_state(node_id: String) -> webrtc::WebRtcSessionState {
    webrtc::session_state(&node_id).await
}

// Returns the room code; host loop runs until leave_room, signalling connects via `audio://webrtc_connected`.
#[tauri::command]
pub async fn webrtc_create_room(
    node_id: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
    password_hash: String,
    app: AppHandle,
) -> AppResult<String> {
    let code = signaling::random_room_code();
    webrtc::mark_room(
        &node_id,
        opus_bitrate,
        opus_application,
        "hosting",
        Some(code.clone()),
    );

    let code_clone = code.clone();
    let task_node_id = node_id.clone();
    let handle = tokio::spawn(async move {
        if let Err(e) = signaling::host_loop(
            code_clone,
            password_hash,
            task_node_id.clone(),
            opus_bitrate,
            opus_application,
        )
        .await
        {
            tracing::error!("signaling host: {e}");
            let _ = app.emit(
                "audio://webrtc_error",
                serde_json::json!({ "nodeId": task_node_id, "error": e.to_string() }),
            );
        }
    });
    webrtc::set_signaling_task(&node_id, handle);

    Ok(code)
}

// Runs in background; result arrives via `audio://webrtc_connected` or `audio://webrtc_error`.
#[tauri::command]
pub async fn webrtc_join_room(
    node_id: String,
    room_code: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
    password_hash: String,
    app: AppHandle,
) -> AppResult<()> {
    webrtc::mark_room(&node_id, opus_bitrate, opus_application, "joining", None);
    let node_id_clone = node_id.clone();
    let handle = tokio::spawn(async move {
        if let Err(e) = signaling::guest_join(
            room_code,
            password_hash,
            node_id_clone.clone(),
            opus_bitrate,
            opus_application,
        )
        .await
        {
            tracing::error!("signaling guest: {e}");
            let _ = app.emit(
                "audio://webrtc_error",
                serde_json::json!({ "nodeId": node_id_clone, "error": e.to_string() }),
            );
        }
    });
    webrtc::set_signaling_task(&node_id, handle);

    Ok(())
}

#[tauri::command]
pub async fn webrtc_leave_room(node_id: String) {
    webrtc::leave_room(&node_id).await;
}
