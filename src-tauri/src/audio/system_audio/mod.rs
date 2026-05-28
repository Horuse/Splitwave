use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AudioApplication {
    #[serde(rename = "bundleId")]
    pub bundle_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{list_audio_applications, load_app_icons};

#[cfg(not(target_os = "macos"))]
mod linux;
#[cfg(not(target_os = "macos"))]
pub use linux::{list_audio_applications, load_app_icons};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_audio_apps_without_hanging() {
        let apps = list_audio_applications().expect("apps");
        println!("found {} audio application(s):", apps.len());
        for a in &apps {
            println!("  - {} ({})", a.name, a.bundle_id);
        }
    }
}
