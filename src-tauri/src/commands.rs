use std::sync::mpsc;

use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use crate::audio::device::{self, DeviceInfo};
use crate::audio::engine::Command;
use crate::audio::graph::GraphSpec;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

const STATE_EVENT: &str = "audio://state";

#[tauri::command]
pub fn list_input_devices() -> AppResult<Vec<DeviceInfo>> {
    device::list_inputs()
}

#[tauri::command]
pub fn list_output_devices() -> AppResult<Vec<DeviceInfo>> {
    device::list_outputs()
}

#[tauri::command]
pub fn start_pipeline(
    graph: GraphSpec,
    state: State<'_, AppState>,
    app: AppHandle,
) -> AppResult<()> {
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
        let _ = app.emit(STATE_EVENT, json!({ "kind": "started" }));
    }
    result
}

#[tauri::command]
pub fn stop_pipeline(state: State<'_, AppState>, app: AppHandle) -> AppResult<()> {
    let (reply_tx, reply_rx) = mpsc::channel();
    state
        .audio_tx
        .send(Command::Stop { reply: reply_tx })
        .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
    let result = reply_rx
        .recv()
        .map_err(|_| AppError::Stream("audio thread reply lost".into()))?;
    if result.is_ok() {
        let _ = app.emit(STATE_EVENT, json!({ "kind": "stopped" }));
    }
    result
}
