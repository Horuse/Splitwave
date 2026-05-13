mod audio;
mod commands;
mod error;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init()
        .ok();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::spawn())
        .invoke_handler(tauri::generate_handler![
            commands::list_input_devices,
            commands::list_output_devices,
            commands::list_audio_applications,
            commands::device_info,
            commands::check_screen_recording_permission,
            commands::start_pipeline,
            commands::stop_pipeline,
            commands::update_effect,
            commands::get_device_volume,
            commands::set_device_volume,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
