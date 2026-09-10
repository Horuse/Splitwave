use std::collections::HashMap;

use tracing::info;

use crate::audio::system_audio::{self, AudioApplication};
use crate::error::AppResult;

#[tauri::command]
pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    let apps = system_audio::list_audio_applications()?;
    info!(count = apps.len(), "audio applications listed");
    Ok(apps)
}

#[tauri::command]
pub fn get_app_icons(bundle_ids: Vec<String>) -> HashMap<String, String> {
    info!(count = bundle_ids.len(), "loading app icons");
    let icons = system_audio::load_app_icons(bundle_ids);
    info!(loaded = icons.len(), "app icons loaded");
    icons
}
