use tracing::error;

use crate::error::{AppError, AppResult};

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
        crate::audio::plugins::main_thread::run(move || {
            crate::audio::plugins::editor::open(&id, &title)
        })
    })
    .await
    .map_err(|_| AppError::Plugin(format!("editor task for {node_id} failed")))?
    .map_err(|e| AppError::Plugin(format!("editor task for {node_id} failed: {e}")))?
    .map_err(AppError::Plugin);
    if let Err(e) = &r {
        error!(node_id, error = %e, "open_plugin_editor failed");
    }
    r
}

#[tauri::command]
pub async fn close_plugin_editor(node_id: String) -> AppResult<()> {
    let id = node_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::audio::plugins::main_thread::run(move || crate::audio::plugins::editor::close(&id))
    })
    .await
    .map_err(|_| AppError::Plugin(format!("editor close task for {node_id} failed")))?
    .map_err(|e| AppError::Plugin(format!("editor close task for {node_id} failed: {e}")))?
    .map_err(AppError::Plugin)
}

/// Serializes a running plugin's state to base64 so the FE can persist it in
/// the node's data. Returns null when the plugin isn't running or has no state.
#[tauri::command]
pub async fn get_plugin_state(node_id: String) -> AppResult<Option<String>> {
    let id = node_id.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        crate::audio::plugins::main_thread::run(move || {
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
    })
    .await;
    match res {
        Ok(Ok(state)) => Ok(state),
        Ok(Err(e)) => {
            error!(node_id, error = %e, "get_plugin_state task failed");
            Ok(None)
        }
        Err(_) => {
            error!(node_id, "get_plugin_state worker failed");
            Ok(None)
        }
    }
}

/// Automatable parameters of a running plugin for the node UI. Empty when the
/// plugin is not running, does not advertise parameters, or on error.
#[tauri::command]
pub async fn get_plugin_params(
    node_id: String,
) -> AppResult<Vec<crate::audio::plugins::PluginParamInfo>> {
    let id = node_id.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        crate::audio::plugins::main_thread::run(move || {
            crate::audio::plugins::registry::for_node(&id)
                .map(|h| h.params(&id))
                .unwrap_or_default()
        })
    })
    .await;
    match res {
        Ok(Ok(params)) => Ok(params),
        Ok(Err(e)) => {
            error!(node_id, error = %e, "get_plugin_params task failed");
            Ok(Vec::new())
        }
        Err(_) => {
            error!(node_id, "get_plugin_params worker failed");
            Ok(Vec::new())
        }
    }
}

/// Which plugin a node is actually running and whether it can show an editor.
/// The node waits on this after a change: a rebuild is not instant, and acting
/// on the outgoing plugin opens the wrong editor.
#[tauri::command]
pub async fn plugin_status(node_id: String) -> AppResult<crate::audio::plugins::PluginStatus> {
    let id = node_id.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        crate::audio::plugins::main_thread::run(move || {
            crate::audio::plugins::registry::for_node(&id)
                .map(|h| h.status(&id))
                .unwrap_or_default()
        })
    })
    .await;
    match res {
        Ok(Ok(status)) => Ok(status),
        Ok(Err(e)) => {
            error!(node_id, error = %e, "plugin_status task failed");
            Ok(Default::default())
        }
        Err(_) => {
            error!(node_id, "plugin_status worker failed");
            Ok(Default::default())
        }
    }
}
