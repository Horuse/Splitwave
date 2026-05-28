use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use objc2_app_kit::{NSRunningApplication, NSWorkspace};

use super::AudioApplication;
use crate::error::AppResult;

fn bundle_path_cache() -> &'static Mutex<HashMap<String, PathBuf>> {
    static C: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn icon_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    let workspace = NSWorkspace::sharedWorkspace();
    let apps = workspace.runningApplications();
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(apps.len());
    let mut paths = bundle_path_cache().lock().unwrap();

    for app in apps.iter() {
        let Some(bundle_id) = bundle_identifier(&app) else { continue };
        let name = localized_name(&app).unwrap_or_else(|| bundle_id.clone());
        if !paths.contains_key(&bundle_id) {
            if let Some(path) = bundle_path(&app) {
                paths.insert(bundle_id.clone(), path);
            }
        }
        if seen.insert(bundle_id.clone()) {
            out.push(AudioApplication { bundle_id, name, icon: None });
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

pub fn load_app_icons(bundle_ids: Vec<String>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut to_load: Vec<(String, PathBuf)> = Vec::new();

    {
        let paths = bundle_path_cache().lock().unwrap();
        let icons = icon_cache().lock().unwrap();
        for id in &bundle_ids {
            if let Some(cached) = icons.get(id) {
                if let Some(icon) = cached {
                    result.insert(id.clone(), icon.clone());
                }
                continue;
            }
            if let Some(path) = paths.get(id) {
                to_load.push((id.clone(), path.clone()));
            }
        }
    }

    let loaded: Vec<(String, Option<String>)> = to_load
        .iter()
        .map(|(id, path)| (id.clone(), icon_from_bundle(path)))
        .collect();

    {
        let mut icons = icon_cache().lock().unwrap();
        for (id, icon) in loaded {
            if let Some(ref png) = icon {
                result.insert(id.clone(), png.clone());
            }
            icons.insert(id, icon);
        }
    }
    result
}

fn bundle_path(app: &NSRunningApplication) -> Option<PathBuf> {
    let url = app.bundleURL()?;
    let path = url.path()?;
    Some(PathBuf::from(path.to_string()))
}

fn bundle_identifier(app: &NSRunningApplication) -> Option<String> {
    app.bundleIdentifier().map(|s| s.to_string())
}

fn localized_name(app: &NSRunningApplication) -> Option<String> {
    app.localizedName().map(|s| s.to_string())
}

fn icon_from_bundle(bundle: &std::path::Path) -> Option<String> {
    let resources = bundle.join("Contents/Resources");
    let bundle_stem = bundle.file_stem()?.to_str()?.to_string();
    let from_plist = icon_name_from_plist(&bundle.join("Contents/Info.plist"));

    let mut candidates: Vec<String> = Vec::new();
    if let Some(name) = from_plist {
        candidates.push(name.trim_end_matches(".icns").to_string());
    }
    candidates.push(bundle_stem);
    candidates.push("AppIcon".to_string());
    candidates.push("app".to_string());
    candidates.dedup();

    for name in &candidates {
        if name.is_empty() {
            continue;
        }
        let path = resources.join(format!("{}.icns", name));
        if let Ok(data) = std::fs::read(&path) {
            if let Some(png) = icns_png(&data) {
                return Some(STANDARD.encode(&png));
            }
        }
    }
    None
}

fn icon_name_from_plist(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let after_key = &text[text.find("CFBundleIconFile")?..];
    let start = after_key.find("<string>")? + "<string>".len();
    let end = after_key[start..].find("</string>")?;
    let name = after_key[start..start + end].trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

fn icns_png(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 8 || &data[..4] != b"icns" {
        return None;
    }
    const ORDER: &[&[u8; 4]] = &[b"ic08", b"ic07", b"ic09", b"ic10", b"ic14", b"ic13"];
    let mut best: Option<(usize, &[u8])> = None;
    let mut pos = 8usize;
    while pos + 8 <= data.len() {
        let tag = &data[pos..pos + 4];
        let size =
            u32::from_be_bytes(data[pos + 4..pos + 8].try_into().ok()?) as usize;
        if size < 8 {
            break;
        }
        let end = pos + size;
        if end > data.len() {
            break;
        }
        let chunk = &data[pos + 8..end];
        if let Some(rank) = ORDER.iter().position(|t| t.as_slice() == tag) {
            if chunk.starts_with(b"\x89PNG") {
                if best.as_ref().map_or(true, |(r, _)| rank < *r) {
                    best = Some((rank, chunk));
                }
            }
        }
        pos = end;
    }
    best.map(|(_, png)| png.to_vec())
}
