use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tracing::{error, info};

use crate::audio::device::{self, DeviceInfo, DeviceKind, NativeDeviceInfo};
use crate::audio::engine::Command;
use crate::audio::graph::{GraphSpec, MicrophoneArrayData, OpusApplication};
use crate::audio::permission::{self, CapturePermission};
use crate::audio::system_audio::{self, AudioApplication};
use crate::audio::virtual_device::{self, VirtualDeviceConfig, VirtualDriverStatus};
use crate::audio::{signaling, webrtc};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const STATE_EVENT: &str = "audio://state";

/// Device open/close legitimately costs hundreds of ms; past this the thread is wedged.
const AUDIO_REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Waits off the main thread: Tauri runs sync commands there, freezing the webview.
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

/// Scans standard install directories for hostable plugins. Loading foreign
/// dylibs blocks and can be slow, so it runs off the main thread.
#[tauri::command]
pub async fn scan_plugins() -> AppResult<Vec<crate::audio::plugins::PluginDescriptor>> {
    tauri::async_runtime::spawn_blocking(crate::audio::plugins::scan_all)
        .await
        .map_err(|_| AppError::Plugin("plugin scan task failed".into()))
}

#[tauri::command]
pub async fn open_plugin_editor(node_id: String, title: String) -> AppResult<()> {
    let id = node_id.clone();
    let r = tauri::async_runtime::spawn_blocking(move || {
        crate::audio::plugins::editor::open(&id, &title)
    })
    .await
    .map_err(|_| AppError::Plugin(format!("editor task for {node_id} failed")))?
    .map_err(AppError::Plugin);
    if let Err(e) = &r {
        // The node id has to be re-supplied here: it was moved into the task.
        error!(node_id, error = %e, "open_plugin_editor failed");
    }
    r
}

/// Serializes a running plugin's state to base64 so the FE can persist it in
/// the node's data. Returns null when the plugin isn't running or has no state.
#[tauri::command]
pub async fn get_plugin_state(node_id: String) -> AppResult<Option<String>> {
    let id = node_id.clone();
    Ok(tauri::async_runtime::spawn_blocking(move || {
        // A format without state persistence reports it; the node then has
        // nothing to store, which is not the same as an empty state.
        crate::audio::plugins::registry::for_node(&id).and_then(|h| match h.save_state(&id) {
            Ok(state) => state,
            Err(unsupported) => {
                tracing::debug!(
                    ?unsupported.format,
                    capability = unsupported.capability,
                    "plugin state not persisted"
                );
                None
            }
        })
    })
    .await
    .unwrap_or_else(|_| {
        error!(node_id, "get_plugin_state task failed");
        None
    }))
}

/// Automatable parameters of a running plugin for the node UI. Empty when the
/// plugin isn't running or exposes no parameters.
#[tauri::command]
pub async fn get_plugin_params(
    node_id: String,
) -> AppResult<Vec<crate::audio::plugins::PluginParamInfo>> {
    let id = node_id.clone();
    Ok(tauri::async_runtime::spawn_blocking(move || {
        crate::audio::plugins::registry::for_node(&id)
            .map(|h| h.params(&id))
            .unwrap_or_default()
    })
    .await
    .unwrap_or_else(|_| {
        error!(node_id, "get_plugin_params task failed");
        Vec::new()
    }))
}

/// Which plugin a node is actually running and whether it can show an editor.
/// The node waits on this after a change: a rebuild is not instant, and acting
/// on the outgoing plugin opens the wrong editor.
#[tauri::command]
pub async fn plugin_status(node_id: String) -> AppResult<crate::audio::plugins::PluginStatus> {
    let id = node_id.clone();
    Ok(tauri::async_runtime::spawn_blocking(move || {
        crate::audio::plugins::registry::for_node(&id)
            .map(|h| h.status(&id))
            .unwrap_or_default()
    })
    .await
    .unwrap_or_else(|_| {
        error!(node_id, "plugin_status task failed");
        Default::default()
    }))
}

/// Persisted crash reports from previous runs, cleared as they are read.
#[tauri::command]
pub fn take_crash_reports() -> Vec<serde_json::Value> {
    crate::take_crash_reports()
}

#[tauri::command]
pub fn get_logs() -> Vec<crate::logs::LogLine> {
    crate::logs::snapshot()
}

#[tauri::command]
pub fn clear_logs() {
    crate::logs::clear();
}

/// Dev-only: panics on the main thread to exercise the crash-persistence path
/// (a real panic, not a faked event). Crashes the app on purpose.
#[tauri::command]
pub fn debug_panic(app: AppHandle) {
    let _ = app.run_on_main_thread(|| {
        panic!("debug: intentional test panic");
    });
}

#[tauri::command]
pub async fn close_plugin_editor(node_id: String) -> AppResult<()> {
    let id = node_id.clone();
    tauri::async_runtime::spawn_blocking(move || crate::audio::plugins::editor::close(&id))
        .await
        .map_err(|_| AppError::Plugin(format!("editor task for {node_id} failed")))?
        .map_err(AppError::Plugin)
}

#[tauri::command]
pub async fn play_cue(device_id: String, muted: bool, gain: f32, beep: bool) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::audio::pipeline::play_cue(&device_id, muted, gain, beep)
    })
    .await
    .map_err(|_| AppError::Stream("cue task failed".into()))?
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
pub async fn calibrate_microphone_array(
    data: MicrophoneArrayData,
) -> AppResult<MicrophoneArrayData> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::audio::pipeline::calibrate_microphone_array(data)
    })
    .await
    .map_err(|_| AppError::Stream("Microphone Array calibration task failed".into()))?
}

#[tauri::command]
pub fn check_capture_permission() -> CapturePermission {
    let state = permission::capture();
    info!(?state, "capture permission checked");
    state
}

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    std::fs::metadata(path).is_ok()
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
pub fn get_device_volume(
    kind: DeviceKind,
    name: String,
) -> Option<crate::audio::volume::DeviceVolume> {
    crate::audio::volume::device_volume(kind, &name)
}

/// Starts emitting `audio://device_volume` for this device until unwatched.
#[tauri::command]
pub fn watch_device_volume(kind: DeviceKind, name: String, app: AppHandle) -> AppResult<()> {
    crate::audio::volume::watch_device_volume(&app, kind, name)
}

#[tauri::command]
pub fn unwatch_device_volume(kind: DeviceKind, name: String) {
    crate::audio::volume::unwatch_device_volume(kind, name);
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
pub async fn set_microphone_array_audition(
    node_id: String,
    mode: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let tx = state.audio_tx.clone();
    audio_request(tx, move |reply| Command::SetMicrophoneArrayAudition {
        node_id,
        mode,
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
pub async fn output_latency_ms(state: State<'_, AppState>) -> AppResult<u32> {
    let tx = state.audio_tx.clone();
    audio_request(tx, |reply| Command::OutputLatencyMs { reply }).await
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
    // Reloading the driver yanks its devices; a pipeline holding one wedges mid-call.
    let tx = state.audio_tx.clone();
    let stopped = match audio_request(tx, |reply| Command::Stop { reply })
        .await
        .map_err(|e| e.to_string())?
    {
        Ok(()) => true,
        // An idle engine already satisfies what Stop is here to guarantee.
        Err(AppError::NotRunning) => false,
        Err(e) => return Err(e.to_string()),
    };
    if stopped {
        let _ = app.emit(STATE_EVENT, json!({ "kind": "stopped" }));
    }
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
    let report = match configured_updater(&app) {
        Ok(updater) => match updater.check().await {
            Ok(Some(u)) => format!("check succeeded; update {} is available", u.version),
            Ok(None) => "check succeeded; no update available".to_string(),
            Err(e) => format_error_chain(&e),
        },
        Err(e) => e,
    };
    error!(diagnostic = %report, "update check diagnostic");
    report
}

// Bundled roots so update checks succeed even when the host trust store isn't
// visible to the process (sandboxed AppImage/Flatpak). `configure_client` runs
// on the updater's own reqwest builder, and the returned `Update` carries the
// same client into its download. Linux-only; macOS/Windows use the platform
// verifier (keychain / Windows store).
#[cfg(target_os = "linux")]
fn updater_tls_config() -> rustls::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = rustls::crypto::ring::default_provider();
    rustls::ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .expect("ring supports TLS 1.2/1.3")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

fn configured_updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    use tauri_plugin_updater::UpdaterExt;
    #[cfg(target_os = "linux")]
    let builder = app
        .updater_builder()
        .configure_client(|b| b.tls_backend_preconfigured(updater_tls_config()));
    #[cfg(not(target_os = "linux"))]
    let builder = app.updater_builder();
    builder.build().map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadata {
    rid: tauri::ResourceId,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    raw_json: serde_json::Value,
}

/// Mirrors `plugin:updater|check` but with bundled roots wired into the HTTP
/// client; the plugin's own `check` command can't be configured.
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<Option<UpdateMetadata>, String> {
    use tauri::Manager;
    let updater = configured_updater(&app)?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let current_version = update.current_version.clone();
    let version = update.version.clone();
    let body = update.body.clone();
    let raw_json = update.raw_json.clone();
    let rid = app.resources_table().add(update);
    Ok(Some(UpdateMetadata {
        rid,
        current_version,
        version,
        date: None,
        body,
        raw_json,
    }))
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
    crate::audio::netaudio::receiver::stats(&node_id).map(
        |(bytes, packets, lost, channels, buffer_ms)| NetReceiverStats {
            bytes,
            packets,
            lost,
            channels,
            buffer_ms,
        },
    )
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
