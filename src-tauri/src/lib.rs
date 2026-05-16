mod audio;
mod commands;
mod error;
mod state;

use std::sync::OnceLock;

use serde_json::json;
use tracing::info;
use state::AppState;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter};

const PANIC_EVENT: &str = "error://panic";
const MENU_EVENT: &str = "menu://action";

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

fn build_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let about_action = MenuItemBuilder::with_id("about", "About Splitwave").build(app)?;
    let check_updates = MenuItemBuilder::with_id("check_updates", "Check for Updates...").build(app)?;
    let undo_action = MenuItemBuilder::with_id("undo", "Undo")
        .accelerator("CmdOrCtrl+Z")
        .build(app)?;
    let redo_action = MenuItemBuilder::with_id("redo", "Redo")
        .accelerator("CmdOrCtrl+Shift+Z")
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
        .copy()
        .paste()
        .select_all()
        .build()?;

    MenuBuilder::new(app)
        .item(&app_submenu)
        .item(&edit_submenu)
        .build()
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let payload = json!({
            "message": info.to_string(),
            "backtrace": backtrace,
            "thread": std::thread::current().name().unwrap_or("<unnamed>"),
            "version": env!("CARGO_PKG_VERSION"),
        });
        if let Some(h) = APP_HANDLE.get() {
            let _ = h.emit(PANIC_EVENT, payload);
        }
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_panic_hook();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init()
        .ok();

    tauri::Builder::default()
        .setup(|app| {
            info!("app started");
            let handle = app.handle().clone();
            let _ = APP_HANDLE.set(handle.clone());

            let menu = build_menu(&handle)?;
            app.set_menu(menu)?;
            app.on_menu_event(|app, event| {
                let _ = app.emit(MENU_EVENT, event.id().0.as_str());
            });

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .manage(AppState::spawn())
        .invoke_handler(tauri::generate_handler![
            commands::list_input_devices,
            commands::list_output_devices,
            commands::list_audio_applications,
            commands::get_app_icons,
            commands::virtual_driver_status,
            commands::install_virtual_driver,
            commands::uninstall_virtual_driver,
            commands::apply_virtual_devices,
            commands::device_info,
            commands::check_screen_recording_permission,
            commands::start_pipeline,
            commands::stop_pipeline,
            commands::reconcile_pipeline,
            commands::update_effect,
            commands::seek_audio_file,
            commands::set_audio_file_loop,
            commands::get_device_volume,
            commands::set_device_volume,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
