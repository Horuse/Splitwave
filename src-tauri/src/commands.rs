use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tracing::{error, info};

use crate::audio::device::{self, DeviceInfo, DeviceKind, NativeDeviceInfo};
use crate::audio::engine::Command;
use crate::audio::graph::{GraphSpec, OpusApplication};
use crate::audio::permission::{self, CapturePermission};
use crate::audio::{signaling, webrtc};
use crate::audio::system_audio::{self, AudioApplication};
use crate::audio::virtual_device::{self, VirtualDeviceConfig, VirtualDriverStatus};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const STATE_EVENT: &str = "audio://state";

/// Opening or closing devices legitimately costs hundreds of ms. Past this the
/// audio thread is wedged -- typically a CoreAudio call into a device that
/// disappeared mid-reconfigure -- and every later request would queue behind it.
const AUDIO_REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Sends a command to the audio thread and waits for its reply off the main
/// thread. Tauri runs sync commands on the main thread, so waiting there froze
/// the whole webview for as long as the reconfigure took.
async fn audio_request<T, F>(tx: Sender<Command>, make: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(Sender<T>) -> Command + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(make(reply_tx))
            .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
        match reply_rx.recv_timeout(AUDIO_REPLY_TIMEOUT) {
            Ok(v) => Ok(v),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AppError::Stream(format!(
                "audio thread did not respond within {}s",
                AUDIO_REPLY_TIMEOUT.as_secs()
            ))),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(AppError::Stream("audio thread reply lost".into()))
            }
        }
    })
    .await
    .map_err(|_| AppError::Stream("audio request task failed".into()))?
}

#[tauri::command]
pub fn list_input_devices() -> AppResult<Vec<DeviceInfo>> {
    let devices = device::list_inputs()?;
    info!(count = devices.len(), "input devices listed");
    Ok(devices)
}

#[tauri::command]
pub fn list_output_devices() -> AppResult<Vec<DeviceInfo>> {
    let devices = device::list_outputs()?;
    info!(count = devices.len(), "output devices listed");
    Ok(devices)
}

#[tauri::command]
pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    let apps = system_audio::list_audio_applications()?;
    info!(count = apps.len(), "audio applications listed");
    Ok(apps)
}

#[tauri::command]
pub fn get_app_icons(bundle_ids: Vec<String>) -> std::collections::HashMap<String, String> {
    info!(count = bundle_ids.len(), "loading app icons");
    let icons = system_audio::load_app_icons(bundle_ids);
    info!(loaded = icons.len(), "app icons loaded");
    icons
}

#[tauri::command]
pub fn device_info(kind: DeviceKind, name: String) -> AppResult<NativeDeviceInfo> {
    device::device_info(kind, &name)
}

#[tauri::command]
pub fn check_capture_permission() -> CapturePermission {
    let state = permission::capture();
    info!(?state, "capture permission checked");
    state
}

#[tauri::command]
pub async fn start_pipeline(
    graph: GraphSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    info!(nodes = graph.nodes.len(), "starting pipeline");
    let valid = graph.validate()?;
    let tx = state.audio_tx.clone();
    let spawned = app.clone();
    let result = audio_request(tx, move |reply| Command::Start {
        graph: valid,
        app: spawned,
        reply,
    })
    .await?;
    if result.is_ok() {
        info!("pipeline started");
        let _ = app.emit(STATE_EVENT, json!({ "kind": "started" }));
    }
    result
}

#[tauri::command]
pub async fn update_effect(
    node_id: String,
    data: serde_json::Value,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let tx = state.audio_tx.clone();
    audio_request(tx, move |reply| Command::UpdateEffect {
        node_id,
        data,
        reply,
    })
    .await?
}

#[tauri::command]
pub fn get_device_volume(kind: DeviceKind, name: String) -> Option<f32> {
    crate::audio::volume::device_volume(kind, &name)
}

#[tauri::command]
pub fn set_device_volume(kind: DeviceKind, name: String, scalar: f32) -> AppResult<()> {
    if crate::audio::volume::set_device_volume(kind, &name, scalar) {
        Ok(())
    } else {
        Err(AppError::Device(format!(
            "device {name:?} has no settable {kind:?} volume"
        )))
    }
}

#[tauri::command]
pub async fn reconcile_pipeline(
    graph: GraphSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    info!(nodes = graph.nodes.len(), "reconciling pipeline");
    let valid = graph.validate()?;
    let tx = state.audio_tx.clone();
    audio_request(tx, move |reply| Command::Reconcile {
        graph: valid,
        app,
        reply,
    })
    .await?
}

#[tauri::command]
pub async fn seek_audio_file(
    node_id: String,
    frame: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let tx = state.audio_tx.clone();
    audio_request(tx, move |reply| Command::SeekAudioFile {
        node_id,
        frame,
        reply,
    })
    .await?
}

#[tauri::command]
pub async fn set_audio_file_loop(
    node_id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let tx = state.audio_tx.clone();
    audio_request(tx, move |reply| Command::SetAudioFileLoop {
        node_id,
        enabled,
        reply,
    })
    .await?
}

#[tauri::command]
pub async fn set_audio_file_paused(
    node_id: String,
    paused: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let tx = state.audio_tx.clone();
    audio_request(tx, move |reply| Command::SetAudioFilePaused {
        node_id,
        paused,
        reply,
    })
    .await?
}

#[tauri::command]
pub async fn set_input_volume(
    node_id: String,
    scalar: f32,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let tx = state.audio_tx.clone();
    audio_request(tx, move |reply| Command::SetInputVolume {
        node_id,
        scalar,
        reply,
    })
    .await?
}

#[tauri::command]
pub async fn is_pipeline_running(state: State<'_, AppState>) -> AppResult<bool> {
    let tx = state.audio_tx.clone();
    audio_request(tx, |reply| Command::IsRunning { reply }).await
}

#[tauri::command]
pub fn virtual_driver_status() -> VirtualDriverStatus {
    virtual_device::status()
}

#[tauri::command]
pub fn install_virtual_driver(app: AppHandle) -> Result<(), String> {
    virtual_device::install(&app)
}

#[tauri::command]
pub fn uninstall_virtual_driver() -> Result<(), String> {
    virtual_device::uninstall()
}

#[tauri::command]
pub async fn apply_virtual_devices(
    devices: Vec<VirtualDeviceConfig>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    info!(count = devices.len(), "applying virtual devices");
    // Reloading the driver yanks its devices out of CoreAudio. A pipeline still
    // holding one wedges the audio thread mid-call, so tear it down first and
    // let the user restart once the new device set is published.
    let tx = state.audio_tx.clone();
    audio_request(tx, |reply| Command::Stop { reply })
        .await
        .and_then(|r| r)
        .map_err(|e| e.to_string())?;
    let _ = app.emit(STATE_EVENT, json!({ "kind": "stopped" }));
    tauri::async_runtime::spawn_blocking(move || virtual_device::apply_virtual_devices(devices))
        .await
        .map_err(|_| "virtual device task failed".to_string())?
}

#[tauri::command]
pub async fn stop_pipeline(state: State<'_, AppState>, app: AppHandle) -> AppResult<()> {
    info!("stopping pipeline");
    let tx = state.audio_tx.clone();
    let result = audio_request(tx, |reply| Command::Stop { reply }).await?;
    if result.is_ok() {
        info!("pipeline stopped");
        let _ = app.emit(STATE_EVENT, json!({ "kind": "stopped" }));
    }
    result
}

// Updater errors serialize Display-only, hiding reqwest's cause; unwind source()+Debug.
#[tauri::command]
pub async fn diagnose_update_error(app: AppHandle) -> String {
    use tauri_plugin_updater::UpdaterExt;
    let report = match app.updater() {
        Ok(updater) => match updater.check().await {
            Ok(Some(u)) => format!("check succeeded; update {} is available", u.version),
            Ok(None) => "check succeeded; no update available".to_string(),
            Err(e) => format_error_chain(&e),
        },
        Err(e) => format_error_chain(&e),
    };
    error!(diagnostic = %report, "update check diagnostic");
    report
}

fn format_error_chain<E: std::error::Error>(err: &E) -> String {
    let mut out = format!("{err}\ndebug: {err:?}");
    let mut src = std::error::Error::source(err);
    let mut depth = 0;
    while let Some(s) = src {
        out.push_str(&format!("\ncaused by [{depth}]: {s}\n  debug: {s:?}"));
        src = s.source();
        depth += 1;
    }
    out
}

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
pub fn webrtc_peer_pings(node_id: String) -> std::collections::HashMap<String, u32> {
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
pub fn webrtc_peer_stats(node_id: String) -> std::collections::HashMap<String, PeerStats> {
    webrtc::peer_stats(&node_id)
        .into_iter()
        .map(|(id, (ping_ms, packets, lost))| (id, PeerStats { ping_ms, packets, lost }))
        .collect()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetReceiverStats {
    pub bytes: u64,
    pub packets: u64,
    pub lost: u64,
}

/// Direct-IP receive stats: cumulative bytes, packets, and lost packets
/// (windowed into a recent loss ratio / rate on the frontend).
#[tauri::command]
pub fn net_receiver_stats(node_id: String) -> Option<NetReceiverStats> {
    crate::audio::netaudio::receiver::stats(&node_id)
        .map(|(bytes, packets, lost)| NetReceiverStats { bytes, packets, lost })
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
