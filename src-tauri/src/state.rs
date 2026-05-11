use std::sync::mpsc::Sender;
use std::thread;

use crate::audio::engine::{self, Command};

/// Shared app state. Holds the sender side of a channel to the audio thread.
/// The audio thread owns the streams (cpal::Stream is !Send on macOS).
pub struct AppState {
    pub audio_tx: Sender<Command>,
}

impl AppState {
    pub fn spawn() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<Command>();
        thread::Builder::new()
            .name("audio".into())
            .spawn(move || engine::run(rx))
            .expect("spawn audio thread");
        Self { audio_tx: tx }
    }
}
