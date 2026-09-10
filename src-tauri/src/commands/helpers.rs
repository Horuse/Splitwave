use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use crate::audio::engine::Command;
use crate::error::{AppError, AppResult};

pub(super) const STATE_EVENT: &str = "audio://state";

/// Device open/close legitimately costs hundreds of ms; past this the thread is wedged.
const AUDIO_REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Waits off the main thread: Tauri runs sync commands there, freezing the webview.
pub(super) async fn audio_request<T, F>(tx: Sender<Command>, make: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(Sender<T>) -> Command + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(make(reply_tx))
            .map_err(|_| AppError::Stream("audio thread is gone".into()))?;
        match reply_rx.recv_timeout(AUDIO_REPLY_TIMEOUT) {
            Ok(v) => Ok(v),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AppError::Stream(format!(
                "audio thread did not respond within {}s",
                AUDIO_REPLY_TIMEOUT.as_secs()
            ))),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(AppError::Stream("audio thread reply lost".into()))
            }
        }
    })
    .await
    .map_err(|_| AppError::Stream("audio request task failed".into()))?
}
