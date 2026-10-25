mod audio;
pub mod commands;
mod error;
mod logs;
mod native_crash;
mod state;
mod watchdog;

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde_json::json;
use state::AppState;
#[cfg(target_os = "macos")]
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager};
use tracing::info;

const PANIC_EVENT: &str = "error://panic";
#[cfg(target_os = "macos")]
const MENU_EVENT: &str = "menu://action";

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

// Set at startup. Reports are persisted before termination and replayed on the
// next launch because a dying process cannot reliably notify the webview.
static CRASH_FILE: OnceLock<PathBuf> = OnceLock::new();
static SESSION_FILE: OnceLock<PathBuf> = OnceLock::new();
/// Tail of the log of a session that ended uncleanly, attached to its reports.
static PREVIOUS_LOGS: Mutex<Option<String>> = Mutex::new(None);

fn append_report(path: &std::path::Path, payload: &serde_json::Value) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{payload}");
    }
}

fn initialize_crash_reporting(dir: &std::path::Path, home: Option<&std::path::Path>) {
    let crash_file = dir.join("crashes.jsonl");
    let session_file = dir.join("running-session");
    let crash_len = std::fs::metadata(&crash_file).map_or(0, |meta| meta.len());
    let previous_hang = watchdog::take_previous_hang(dir);

    if let Ok(baseline) = std::fs::read_to_string(&session_file) {
        if let Ok(mut logs) = PREVIOUS_LOGS.lock() {
            *logs = logs::previous_session_tail(dir, home);
        }
        let baseline = baseline.trim().parse::<u64>().unwrap_or(0);
        if crash_len <= baseline {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or(0);
            let report = match previous_hang {
                Some(hang) => hang_report(&hang, ts),
                None => json!({
                    "kind": "unexpectedExit",
                    "message": "Splitwave did not shut down cleanly",
                    "backtrace": "No native stack was captured. The process may have been killed by the OS, an out-of-memory condition, or a native crash without a dump.",
                    "thread": "<unknown>",
                    "version": env!("CARGO_PKG_VERSION"),
                    "ts": ts,
                }),
            };
            append_report(&crash_file, &report);
        }
    }

    let current_len = std::fs::metadata(&crash_file).map_or(0, |meta| meta.len());
    let _ = std::fs::write(&session_file, current_len.to_string());
    let _ = CRASH_FILE.set(crash_file.clone());
    let _ = SESSION_FILE.set(session_file);
    if let Err(error) = native_crash::install(&crash_file) {
        tracing::error!(%error, "failed to install native crash handler");
    }
}

fn hang_report(hang: &serde_json::Value, ts: u64) -> serde_json::Value {
    let seconds = hang["stalledMs"].as_u64().unwrap_or(0) / 1000;
    let place = match hang["command"].as_str() {
        Some(command) => format!("in the `{command}` command"),
        None => "outside any IPC command".to_string(),
    };
    json!({
        "kind": "hang",
        "message": "Splitwave stopped responding and was closed",
        "backtrace": format!("The main thread was blocked for at least {seconds} s {place} before the app was closed. A hang captures no stack; the session log shows what led up to it."),
        "thread": "main",
        "version": env!("CARGO_PKG_VERSION"),
        "ts": hang["ts"].as_u64().unwrap_or(ts),
    })
}

fn with_watchdog(
    handler: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    move |invoke| {
        let _command = watchdog::enter_command(invoke.message.command());
        handler(invoke)
    }
}

fn mark_clean_exit() {
    if let Some(path) = SESSION_FILE.get() {
        let _ = std::fs::remove_file(path);
    }
    watchdog::clear_marker();
    logs::flush_file_sink();
}

#[cfg(target_os = "windows")]
pub fn run_windows_vb_cable_helper() -> Option<i32> {
    audio::virtual_device::windows_cable::run_helper()
}

pub fn app_handle() -> Option<&'static AppHandle> {
    APP_HANDLE.get()
}

/// Reads and clears persisted crash reports (best-effort).
pub fn take_crash_reports() -> Vec<serde_json::Value> {
    let Some(path) = CRASH_FILE.get() else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path);
    if let Some(session) = SESSION_FILE.get() {
        let _ = std::fs::write(session, "0");
    }
    let logs = PREVIOUS_LOGS.lock().ok().and_then(|mut l| l.take());
    contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .map(|mut report| {
            if let (Some(logs), Some(fields)) = (&logs, report.as_object_mut()) {
                fields.insert("logs".into(), json!(logs));
            }
            report
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn build_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let about_action = MenuItemBuilder::with_id("about", "About Splitwave").build(app)?;
    let check_updates =
        MenuItemBuilder::with_id("check_updates", "Check for Updates...").build(app)?;
    let undo_action = MenuItemBuilder::with_id("undo", "Undo")
        .accelerator("CmdOrCtrl+Z")
        .build(app)?;
    let redo_action = MenuItemBuilder::with_id("redo", "Redo")
        .accelerator("CmdOrCtrl+Shift+Z")
        .build(app)?;
    // Predefined Copy/Paste/SelectAll swallow their accelerators in AppKit before
    // the webview sees a keydown, so the graph could never bind them. Custom items
    // forward to the frontend, which dispatches to the focused field or the canvas.
    let copy_action = MenuItemBuilder::with_id("copy", "Copy")
        .accelerator("CmdOrCtrl+C")
        .build(app)?;
    let paste_action = MenuItemBuilder::with_id("paste", "Paste")
        .accelerator("CmdOrCtrl+V")
        .build(app)?;
    let select_all_action = MenuItemBuilder::with_id("select_all", "Select All")
        .accelerator("CmdOrCtrl+A")
        .build(app)?;

    let app_submenu = SubmenuBuilder::new(app, "Splitwave")
        .item(&about_action)
        .separator()
        .item(&check_updates)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let edit_submenu = SubmenuBuilder::new(app, "Edit")
        .item(&undo_action)
        .item(&redo_action)
        .separator()
        .cut()
        .item(&copy_action)
        .item(&paste_action)
        .item(&select_all_action)
        .build()?;

    MenuBuilder::new(app)
        .item(&app_submenu)
        .item(&edit_submenu)
        .build()
}

std::thread_local! {
    // Reentrancy guard: re-installing after each plugin load stacks wrapper
    // layers, but only the outermost should persist/emit a given panic.
    static IN_PANIC_HOOK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Installs (or re-installs) the panic hook, chaining to whatever hook is
/// current. Loaded plugins (nih-plug et al.) replace the global hook, so this
/// is called again after each plugin load to keep our persistence outermost.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let outermost = IN_PANIC_HOOK.with(|f| {
            let was = f.get();
            f.set(true);
            !was
        });
        if outermost {
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let payload = json!({
                "kind": "rustPanic",
                "message": info.to_string(),
                "backtrace": backtrace,
                "thread": std::thread::current().name().unwrap_or("<unnamed>"),
                "version": env!("CARGO_PKG_VERSION"),
                "ts": ts,
            });
            // Persist first: a main-thread panic tears the app down before the
            // live event can be delivered, so the file is the only reliable path.
            if let Some(path) = CRASH_FILE.get() {
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    use std::io::Write;
                    let _ = writeln!(f, "{payload}");
                }
            }
            if let Some(h) = APP_HANDLE.get() {
                let _ = h.emit(PANIC_EVENT, payload);
            }
        }
        previous(info);
        if outermost {
            IN_PANIC_HOOK.with(|f| f.set(false));
        }
    }));
}

/// Re-asserts our panic hook as the outermost layer after a plugin has had a
/// chance to install its own.
pub fn reinstall_panic_hook() {
    install_panic_hook();
    if let Err(error) = native_crash::reinstall() {
        tracing::error!(%error, "failed to reinstall native crash handler");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();
    audio::plugins::main_thread::register_main_thread();

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(logs::RingLayer)
        .try_init()
        .ok();

    tauri::Builder::default()
        .setup(|app| {
            info!("app started");
            let handle = app.handle().clone();
            let _ = APP_HANDLE.set(handle.clone());

            #[cfg(target_os = "linux")]
            pipewire::init();

            if let Ok(dir) = handle.path().app_log_dir() {
                let _ = std::fs::create_dir_all(&dir);
                logs::start_file_sink(&dir);
                let home = handle.path().home_dir().ok();
                initialize_crash_reporting(&dir, home.as_deref());
                watchdog::start(handle.clone(), &dir);
                if let Some(file) = CRASH_FILE.get() {
                    info!(path = %file.display(), "crash log");
                }
            }

            if let Ok(dir) = handle.path().app_data_dir() {
                let current = dir.join("pipelines.json");
                let backup = dir.join("pipelines.backup-v1.json");
                if current.exists() && !backup.exists() {
                    if let Ok(bytes) = std::fs::read(&current) {
                        let _ = std::fs::write(&backup, bytes);
                        info!(path = %backup.display(), "created automatic pre-1.2.0 pipeline backup");
                    }
                }
            }

            #[cfg(target_os = "linux")]
            if let Err(error) = audio::virtual_device::restore(&handle) {
                tracing::error!(%error, "failed to restore PipeWire virtual devices");
            }

            // Native menu only on macOS (top menu bar). On Linux GTK renders it
            // as an in-window bar that clashes with the custom titlebar, so the
            // menu actions live in the in-app header instead.
            #[cfg(target_os = "macos")]
            {
                let menu = build_menu(&handle)?;
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| {
                    let _ = app.emit(MENU_EVENT, event.id().0.as_str());
                });
            }

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::spawn())
        .invoke_handler(with_watchdog(tauri::generate_handler![
            commands::scan_plugins,
            commands::open_plugin_editor,
            commands::close_plugin_editor,
            commands::get_plugin_state,
            commands::get_plugin_params,
            commands::plugin_status,
            commands::take_crash_reports,
            commands::get_logs,
            commands::clear_logs,
            commands::log_frontend,
            commands::debug_panic,
            commands::debug_native_crash,
            commands::debug_unexpected_exit,
            commands::debug_hang,
            commands::list_input_devices,
            commands::list_output_devices,
            commands::play_cue,
            commands::list_audio_applications,
            commands::get_app_icons,
            commands::virtual_driver_status,
            commands::windows_virtual_cable_status,
            commands::install_windows_virtual_cable,
            commands::install_virtual_driver,
            commands::uninstall_virtual_driver,
            commands::apply_virtual_devices,
            commands::device_info,
            commands::capture_device_info,
            commands::check_capture_permission,
            commands::path_exists,
            commands::read_file_peaks,
            commands::is_pipeline_running,
            commands::output_latency_ms,
            commands::start_pipeline,
            commands::stop_pipeline,
            commands::reconcile_pipeline,
            commands::update_effect,
            commands::seek_audio_file,
            commands::set_audio_file_loop,
            commands::set_audio_file_paused,
            commands::set_input_volume,
            commands::get_device_volume,
            commands::set_device_volume,
            commands::watch_device_volume,
            commands::unwatch_device_volume,
            commands::diagnose_update_error,
            commands::check_for_updates,
            commands::webrtc_create_offer,
            commands::webrtc_accept_offer,
            commands::webrtc_complete_handshake,
            commands::webrtc_disconnect_peer,
            commands::webrtc_set_peer_muted,
            commands::webrtc_create_room,
            commands::webrtc_join_room,
            commands::webrtc_leave_room,
            commands::webrtc_peer_pings,
            commands::webrtc_peer_stats,
            commands::webrtc_buffer_ms,
            commands::net_receiver_listen,
            commands::net_receiver_release,
            commands::net_receiver_stats,
            commands::net_sender_stats,
            commands::webrtc_set_identity,
            commands::webrtc_session_state,
        ]))
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                mark_clean_exit();
            }
        });
}
