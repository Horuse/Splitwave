use std::sync::mpsc;

use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tracing::{error, info};

use crate::audio::device::{self, DeviceInfo, DeviceKind, NativeDeviceInfo};
use crate::audio::engine::Command;
use crate::audio::graph::{GraphSpec, OpusApplication};
use crate::audio::permission::{self, PermissionState};
use crate::audio::{signaling, webrtc};
use crate::audio::system_audio::{self, AudioApplication};
use crate::audio::virtual_device::{self, VirtualDeviceConfig, VirtualDriverStatus};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const STATE_EVENT: &str = "audio://state";

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
pub fn check_screen_recording_permission() -> PermissionState {
    let state = permission::screen_recording();
    info!(?state, "screen recording permission checked");
    state
}

#[tauri::command]
pub fn start_pipeline(
    graph: GraphSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    info!(nodes = graph.nodes.len(), "starting pipeline");
    let valid = graph.validate()?;
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::Start {
            graph: valid,
            app: app.clone(),
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    let result = reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?;
    if result.is_ok() {
        info!("pipeline started");
        let _ = app.emit(STATE_EVENT, json!({ "kind": "started" }));
    }
    result
}

#[tauri::command]
pub fn update_effect(
    node_id: String,
    data: serde_json::Value,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::UpdateEffect {
            node_id,
            data,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
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
pub fn reconcile_pipeline(
    graph: GraphSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
    info!(nodes = graph.nodes.len(), "reconciling pipeline");
    let valid = graph.validate()?;
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::Reconcile {
            graph: valid,
            app,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn seek_audio_file(
    node_id: String,
    frame: i64,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::SeekAudioFile {
            node_id,
            frame,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn set_audio_file_loop(
    node_id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::SetAudioFileLoop {
            node_id,
            enabled,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn set_audio_file_paused(
    node_id: String,
    paused: bool,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::SetAudioFilePaused {
            node_id,
            paused,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn set_input_volume(
    node_id: String,
    scalar: f32,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::SetInputVolume {
            node_id,
            scalar,
            reply: reply_tx,
        })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?
}

#[tauri::command]
pub fn is_pipeline_running(state: State<'_, AppState>) -> AppResult<bool> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::IsRunning { reply: reply_tx })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))
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
pub fn apply_virtual_devices(devices: Vec<VirtualDeviceConfig>) -> Result<(), String> {
    info!(count = devices.len(), "applying virtual devices");
    virtual_device::apply_virtual_devices(devices)
}

#[tauri::command]
pub fn stop_pipeline(state: State<'_, AppState>, app: AppHandle) -> AppResult<()> {
    info!("stopping pipeline");
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::Stop { reply: reply_tx })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    let result = reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?;
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

/// Stores the local participant name and input count; peers receive them via
/// the ctrl channel's periodic meta message.
#[tauri::command]
pub fn webrtc_set_identity(
    node_id: String,
    name: String,
    channels: u32,
    opus_bitrate: u32,
    opus_application: OpusApplication,
) {
    webrtc::get_or_create(&node_id, opus_bitrate, opus_application);
    webrtc::set_identity(&node_id, name, channels);
}

#[tauri::command]
pub async fn webrtc_session_state(node_id: String) -> webrtc::WebRtcSessionState {
    webrtc::session_state(&node_id).await
}

/// Creates a WebRTC offer and connects to the signaling room as host.
/// Returns the 6-char room code immediately; the actual peer connection
/// completes asynchronously, signalled via `audio://webrtc_connected`.
#[tauri::command]
pub async fn webrtc_create_room(
    node_id: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
    app: AppHandle,
) -> AppResult<String> {
    let code = signaling::random_room_code();
    let (peer_id, offer_sdp) =
        webrtc::create_offer(node_id.clone(), opus_bitrate, opus_application).await?;
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
        match signaling::host_exchange(&code_clone, &peer_id, &offer_sdp).await {
            Ok((_guest_peer_id, answer_sdp)) => {
                // complete_handshake updates display_id to guest_peer_id;
                // webrtc_connected is emitted from DataChannel on_open.
                if let Err(e) =
                    webrtc::complete_handshake(task_node_id.clone(), answer_sdp).await
                {
                    tracing::error!("webrtc complete_handshake: {e}");
                    let _ = app.emit(
                        "audio://webrtc_error",
                        serde_json::json!({ "nodeId": task_node_id, "error": e.to_string() }),
                    );
                }
            }
            Err(e) => {
                tracing::error!("signaling host_exchange: {e}");
                let _ = app.emit(
                    "audio://webrtc_error",
                    serde_json::json!({ "nodeId": task_node_id, "error": e.to_string() }),
                );
            }
        }
    });
    webrtc::set_signaling_task(&node_id, handle);

    Ok(code)
}

/// Guest side: connects to a signaling room, receives the host's offer,
/// completes the WebRTC handshake, and sends the answer back.
/// Runs fully in background; result arrives via `audio://webrtc_connected`
/// or `audio://webrtc_error`.
#[tauri::command]
pub async fn webrtc_join_room(
    node_id: String,
    room_code: String,
    opus_bitrate: u32,
    opus_application: OpusApplication,
    app: AppHandle,
) -> AppResult<()> {
    webrtc::mark_room(&node_id, opus_bitrate, opus_application, "joining", None);
    let node_id_clone = node_id.clone();
    let handle = tokio::spawn(async move {
        let result = signaling::guest_exchange(&room_code, |_host_peer_id, offer_sdp| {
            let nid = node_id_clone.clone();
            async move {
                webrtc::accept_offer(nid, offer_sdp, opus_bitrate, opus_application).await
            }
        })
        .await;

        if let Err(e) = result {
            tracing::error!("signaling guest_exchange: {e}");
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
