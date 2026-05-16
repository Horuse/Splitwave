use std::path::Path;
use std::process::Command;

use tauri::{AppHandle, Manager};
use tracing::{error, info};

// Reject paths that would escape single-quote shell quoting.
fn shell_safe(path: &Path) -> Result<&str, String> {
    let s = path.to_str().ok_or("path is not valid UTF-8")?;
    if s.contains('\'') || s.contains('\n') || s.contains('\r') {
        return Err(format!("path contains unsafe characters: {s}"));
    }
    Ok(s)
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct VirtualDeviceConfig {
    pub id: String,
    pub name: String,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn apply_virtual_devices(devices: Vec<VirtualDeviceConfig>) -> Result<(), String> {
    let dst = Path::new(HAL_DIR).join(DRIVER_NAME);
    if !dst.exists() {
        return Err("virtual driver is not installed".into());
    }

    let mut plist = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <array>\n",
    );
    for d in &devices {
        plist.push_str(&format!(
            "\t<dict>\n\t\t<key>id</key><string>{}</string>\n\t\t<key>name</key><string>{}</string>\n\t</dict>\n",
            xml_escape(&d.id),
            xml_escape(&d.name)
        ));
    }
    plist.push_str("</array>\n</plist>\n");

    let tmp = format!("/tmp/splitwave_devices_{}.plist", std::process::id());
    std::fs::write(&tmp, &plist).map_err(|e| format!("write temp plist: {e}"))?;

    let tmp_safe = shell_safe(Path::new(&tmp))?;
    let script = format!(
        concat!(
            r#"do shell script "mkdir -p '/Library/Audio/Plug-Ins/HAL/Splitwave.driver/Contents/Resources'"#,
            r#" && cp '{src}' '/Library/Audio/Plug-Ins/HAL/Splitwave.driver/Contents/Resources/devices.plist'"#,
            r#"; s=$?; killall -9 coreaudiod 2>/dev/null; exit $s" "#,
            r#"with administrator privileges"#,
        ),
        src = tmp_safe,
    );

    let out = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("osascript failed: {e}"))?;

    let _ = std::fs::remove_file(&tmp);

    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Apply cancelled or failed: {}", msg.trim()));
    }

    info!(count = devices.len(), "virtual devices applied");
    Ok(())
}

const DRIVER_NAME: &str = "Splitwave.driver";
const HAL_DIR: &str = "/Library/Audio/Plug-Ins/HAL";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDriverStatus {
    pub installed: bool,
}

pub fn status() -> VirtualDriverStatus {
    let installed = Path::new(HAL_DIR).join(DRIVER_NAME).exists();
    info!(installed, "virtual driver status");
    VirtualDriverStatus { installed }
}

pub fn install(app: &AppHandle) -> Result<(), String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?;
    let src = resource_dir.join(DRIVER_NAME);

    if !src.exists() {
        return Err(format!("driver bundle not found in app resources: {}", src.display()));
    }

    // Destination is a constant. Only src (resource_dir) is dynamic — validate it.
    let src_safe = shell_safe(&src)?;
    info!(src = src_safe, "installing virtual driver");

    // rm + cp + chown must all succeed; killall is fire-and-forget.
    let script = format!(
        concat!(
            r#"do shell script "mkdir -p '/Library/Audio/Plug-Ins/HAL'"#,
            r#" && rm -rf '/Library/Audio/Plug-Ins/HAL/Splitwave.driver'"#,
            r#" && cp -R '{src}' '/Library/Audio/Plug-Ins/HAL/Splitwave.driver'"#,
            r#" && chown -R root:wheel '/Library/Audio/Plug-Ins/HAL/Splitwave.driver'"#,
            r#"; s=$?; killall -9 coreaudiod 2>/dev/null; exit $s" "#,
            r#"with administrator privileges"#,
        ),
        src = src_safe,
    );

    let out = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("osascript failed: {e}"))?;

    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        error!(msg = msg.trim(), "virtual driver install failed");
        return Err(format!("Installation failed: {}", msg.trim()));
    }

    info!("virtual driver installed");
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let dst = Path::new(HAL_DIR).join(DRIVER_NAME);
    if !dst.exists() {
        return Ok(());
    }

    let script =
        "do shell script \"rm -rf '/Library/Audio/Plug-Ins/HAL/Splitwave.driver'; killall -9 coreaudiod 2>/dev/null; true\" with administrator privileges";

    Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|e| format!("osascript failed: {e}"))?;

    info!("virtual driver uninstalled");
    Ok(())
}
