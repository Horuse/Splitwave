use std::collections::HashMap;

use super::AudioApplication;
use crate::error::AppResult;

pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    Ok(Vec::new())
}

pub fn load_app_icons(_bundle_ids: Vec<String>) -> HashMap<String, String> {
    HashMap::new()
}
