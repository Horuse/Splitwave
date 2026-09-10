use serde_json::json;
use tauri::{AppHandle, Emitter, State};
use tracing::info;

use crate::audio::engine::Command;
use crate::audio::graph::GraphSpec;
use crate::error::AppResult;
use crate::state::AppState;

use super::helpers::{audio_request, STATE_EVENT};

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
pub async fn output_latency_ms(state: State<'_, AppState>) -> AppResult<u32> {
    let tx = state.audio_tx.clone();
    audio_request(tx, |reply| Command::OutputLatencyMs { reply }).await
}
