use tauri::AppHandle;

/// Persisted crash reports from previous runs, cleared as they are read.
#[tauri::command]
pub fn take_crash_reports() -> Vec<serde_json::Value> {
    crate::take_crash_reports()
}

#[tauri::command]
pub fn get_logs() -> Vec<crate::logs::LogLine> {
    crate::logs::snapshot()
}

#[tauri::command]
pub fn clear_logs() {
    crate::logs::clear();
}

/// Dev-only: panics on the main thread to exercise the crash-persistence path
/// (a real panic, not a faked event). Crashes the app on purpose.
#[tauri::command]
pub fn debug_panic(app: AppHandle) {
    #[cfg(debug_assertions)]
    {
        let _ = app.run_on_main_thread(|| {
            panic!("debug: intentional test panic");
        });
    }
    #[cfg(not(debug_assertions))]
    let _ = app;
}

/// Dev-only: exercises the platform native signal/exception crash reporter.
#[tauri::command]
pub fn debug_native_crash() {
    #[cfg(debug_assertions)]
    crate::native_crash::trigger();
}

/// Dev-only: exits without panic or a catchable signal to test the session marker.
#[tauri::command]
pub fn debug_unexpected_exit() {
    #[cfg(debug_assertions)]
    std::process::exit(86);
}
