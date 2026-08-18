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

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{list_audio_applications, load_app_icons};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{list_audio_applications, load_app_icons, pid_for_exe};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[cfg(target_os = "windows")]
    fn is_missing_audio_endpoint(error: &crate::error::AppError) -> bool {
        error.to_string().contains("0x80070490")
    }

    #[cfg(not(target_os = "windows"))]
    fn is_missing_audio_endpoint(_error: &crate::error::AppError) -> bool {
        false
    }

    #[test]
    fn enumerates_audio_apps_without_hanging() {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(list_audio_applications());
        });

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(apps)) => {
                println!("found {} audio application(s):", apps.len());
                for app in &apps {
                    println!("  - {} ({})", app.name, app.bundle_id);
                }
            }
            Ok(Err(error)) if is_missing_audio_endpoint(&error) => {
                println!("audio enumeration completed without a default endpoint: {error}");
            }
            Ok(Err(error)) => panic!("audio enumeration failed: {error}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("audio enumeration did not finish within 5 seconds")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("audio enumeration thread stopped unexpectedly")
            }
        }
    }
}
