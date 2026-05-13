use serde::Serialize;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct AudioApplication {
    #[serde(rename = "bundleId")]
    pub bundle_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[cfg(target_os = "macos")]
pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    Ok(macos::list_running_applications())
}

#[cfg(not(target_os = "macos"))]
pub fn list_audio_applications() -> AppResult<Vec<AudioApplication>> {
    Ok(Vec::new())
}

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashSet;

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSRunningApplication, NSWorkspace,
    };
    use objc2_foundation::NSDictionary;

    use super::AudioApplication;

    pub fn list_running_applications() -> Vec<AudioApplication> {
        let workspace = NSWorkspace::sharedWorkspace();
        let apps = workspace.runningApplications();
        let mut seen = HashSet::new();
        let mut out = Vec::with_capacity(apps.len());
        for app in apps.iter() {
            let Some(bundle_id) = bundle_identifier(&app) else {
                continue;
            };
            let name = localized_name(&app).unwrap_or_else(|| bundle_id.clone());
            // Dedupe — multiple `NSRunningApplication` entries can share a bundle id
            // (helpers, login items). The user just wants one entry per app.
            if seen.insert(bundle_id.clone()) {
                let icon = icon_png_b64(&app);
                out.push(AudioApplication { bundle_id, name, icon });
            }
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out
    }

    fn bundle_identifier(app: &NSRunningApplication) -> Option<String> {
        app.bundleIdentifier().map(|s| s.to_string())
    }

    fn localized_name(app: &NSRunningApplication) -> Option<String> {
        app.localizedName().map(|s| s.to_string())
    }

    // NSImage → TIFF → NSBitmapImageRep is the only Cocoa path to PNG encoding.
    fn icon_png_b64(app: &NSRunningApplication) -> Option<String> {
        let img: objc2::rc::Retained<NSImage> = app.icon()?;
        let tiff = img.TIFFRepresentation()?;
        let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
        let empty_props = NSDictionary::new();
        let png = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &empty_props)
        }?;
        Some(STANDARD.encode(png.to_vec()))
    }
}
