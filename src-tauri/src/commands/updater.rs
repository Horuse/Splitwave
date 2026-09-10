use tauri::AppHandle;
use tracing::error;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadata {
    rid: tauri::ResourceId,
    current_version: String,
    version: String,
    date: Option<String>,
    body: Option<String>,
    raw_json: serde_json::Value,
}

// Updater errors serialize Display-only, hiding reqwest's cause; unwind source()+Debug.
#[tauri::command]
pub async fn diagnose_update_error(app: AppHandle) -> String {
    let report = match configured_updater(&app) {
        Ok(updater) => match updater.check().await {
            Ok(Some(u)) => format!("check succeeded; update {} is available", u.version),
            Ok(None) => "check succeeded; no update available".to_string(),
            Err(e) => format_error_chain(&e),
        },
        Err(e) => e,
    };
    error!(diagnostic = %report, "update check diagnostic");
    report
}

// Bundled roots so update checks succeed even when the host trust store isn't
// visible to the process (sandboxed AppImage/Flatpak). `configure_client` runs
// on the updater's own reqwest builder, and the returned `Update` carries the
// same client into its download. Linux-only; macOS/Windows use the platform
// verifier (keychain / Windows store).
#[cfg(target_os = "linux")]
fn updater_tls_config() -> rustls::ClientConfig {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = rustls::crypto::ring::default_provider();
    rustls::ClientConfig::builder_with_provider(provider.into())
        .with_safe_default_protocol_versions()
        .expect("ring supports TLS 1.2/1.3")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

fn configured_updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    use tauri_plugin_updater::UpdaterExt;
    #[cfg(target_os = "linux")]
    let builder = app
        .updater_builder()
        .configure_client(|b| b.tls_backend_preconfigured(updater_tls_config()));
    #[cfg(not(target_os = "linux"))]
    let builder = app.updater_builder();
    builder.build().map_err(|e| e.to_string())
}

/// Mirrors `plugin:updater|check` but with bundled roots wired into the HTTP
/// client; the plugin's own `check` command can't be configured.
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<Option<UpdateMetadata>, String> {
    use tauri::Manager;
    let updater = configured_updater(&app)?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let current_version = update.current_version.clone();
    let version = update.version.clone();
    let body = update.body.clone();
    let raw_json = update.raw_json.clone();
    let rid = app.resources_table().add(update);
    Ok(Some(UpdateMetadata {
        rid,
        current_version,
        version,
        date: None,
        body,
        raw_json,
    }))
}

fn format_error_chain<E: std::error::Error>(err: &E) -> String {
    let mut out = format!("{err}\ndebug: {err:?}");
    let mut src = std::error::Error::source(err);
    let mut depth = 0;
    while let Some(s) = src {
        out.push_str(&format!("\ncaused by [{depth}]: {s}\n  debug: {s:?}"));
        src = s.source();
        depth += 1;
    }
    out
}
