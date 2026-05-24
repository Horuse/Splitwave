use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;

use tauri::{AppHandle, Manager};
use tracing::{error, info};

use super::{VirtualDeviceConfig, VirtualDriverStatus};

const DRIVER_NAME: &str = "Splitwave.driver";
const HAL_DIR: &str = "/Library/Audio/Plug-Ins/HAL";

// Reject paths that would escape single-quote shell quoting or the enclosing AppleScript string.
fn shell_safe(path: &Path) -> Result<&str, String> {
    let s = path.to_str().ok_or("path is not valid UTF-8")?;
    if s.contains('\'') || s.contains('"') || s.contains('\\') || s.contains('\n') || s.contains('\r') {
        return Err(format!("path contains unsafe characters: {s}"));
    }
    Ok(s)
}

struct TempPlist(std::path::PathBuf);

impl Drop for TempPlist {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_temp_plist(content: &str) -> Result<TempPlist, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let path = std::path::PathBuf::from(format!(
        "/tmp/splitwave_devices_{}_{}.plist",
        std::process::id(),
        nanos
    ));
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("create temp plist: {e}"))?;
    let guard = TempPlist(path);
    { let mut f = f; f.write_all(content.as_bytes()) }
        .map_err(|e| format!("write temp plist: {e}"))?;
    Ok(guard)
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

    let tmp = write_temp_plist(&plist)?;

    const SCRIPT: &str = r#"on run argv
    set src to item 1 of argv
    set dstDir to "/Library/Audio/Plug-Ins/HAL/Splitwave.driver/Contents/Resources"
    set dst to dstDir & "/devices.plist"
    do shell script "mkdir -p " & quoted form of dstDir & " && cp " & quoted form of src & " " & quoted form of dst & " && chmod 644 " & quoted form of dst & "; s=$?; killall -9 coreaudiod 2>/dev/null; exit $s" with administrator privileges
end run"#;

    let out = Command::new("osascript")
        .arg("-e").arg(SCRIPT)
        .arg("--")
        .arg(&tmp.0)
        .output()
        .map_err(|e| format!("osascript failed: {e}"))?;

    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Apply cancelled or failed: {}", msg.trim()));
    }

    info!(count = devices.len(), "virtual devices applied");
    Ok(())
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
