use crate::audio::input_bridge::BroadcastRx;
use crate::error::{AppError, AppResult};

pub struct Capture;

impl Capture {
    pub fn start_system(
        _exclude_current_app: bool,
        _sample_rate: u32,
        _channels: u32,
        _bridge: BroadcastRx,
    ) -> AppResult<Self> {
        Err(AppError::Stream(
            "system-audio capture is not implemented on Windows yet".into(),
        ))
    }

    pub fn start_app(
        _bundle_id: &str,
        _sample_rate: u32,
        _channels: u32,
        _bridge: BroadcastRx,
    ) -> AppResult<Self> {
        Err(AppError::Stream(
            "app-audio capture is not implemented on Windows yet".into(),
        ))
    }
}
