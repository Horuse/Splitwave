use crate::error::{AppError, AppResult};

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    std::fs::metadata(path).is_ok()
}

/// Reads min/max peak bins from a WAV/AIFF file for a requested frame range, so
/// the File Recording node can show the whole file without holding its samples
/// in RAM. Compressed formats return an error and fall back to the live scope.
#[tauri::command]
pub async fn read_file_peaks(
    path: String,
    start_frame: u64,
    frames_per_bin: u32,
    bin_count: u32,
) -> AppResult<crate::audio::encoders::FilePeaks> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::audio::encoders::read_peaks(
            std::path::Path::new(&path),
            start_frame,
            frames_per_bin,
            bin_count,
        )
    })
    .await
    .map_err(|_| AppError::Stream("peak read task failed".into()))?
}
