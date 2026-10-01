//! What a pipeline needs from outside the engine: somewhere to send UI events,
//! and audio devices. The app hands it its window and the system's devices;
//! tests hand it a recorder of events and virtual devices, so a whole pipeline
//! runs where there is no sound hardware at all.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use super::file_reader::ProgressEmitter;
use super::virtual_io::VirtualDevices;

type EventSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

#[derive(Clone)]
pub struct Host {
    events: EventSink,
    devices: Option<Arc<VirtualDevices>>,
}

impl Host {
    /// The app's window for events, the system's devices for audio.
    pub fn app(app: AppHandle) -> Self {
        Self {
            events: Arc::new(move |event, payload| {
                let _ = app.emit(event, payload);
            }),
            devices: None,
        }
    }

    /// `devices` stand in for every microphone, capture and speaker the
    /// graph names; `events` receives what the UI would.
    pub fn with_virtual_devices(
        devices: Arc<VirtualDevices>,
        events: impl Fn(&str, serde_json::Value) + Send + Sync + 'static,
    ) -> Self {
        Self {
            events: Arc::new(events),
            devices: Some(devices),
        }
    }

    pub fn emit(&self, event: &str, payload: serde_json::Value) {
        (self.events)(event, payload);
    }

    pub(super) fn virtual_devices(&self) -> Option<&Arc<VirtualDevices>> {
        self.devices.as_ref()
    }
}

impl ProgressEmitter for Host {
    fn emit_progress(&self, payload: serde_json::Value) {
        self.emit(super::file_reader::PROGRESS_EVENT, payload);
    }
}
