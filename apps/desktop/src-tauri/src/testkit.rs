//! Runs whole pipelines on virtual devices: the engine the app runs, end to
//! end, on machines with no sound hardware (CI). For the tests in `tests/`;
//! nothing in the app calls it.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::audio::graph::{GraphSpec, RecordingFormat};
use crate::audio::pipeline::{ActivePipeline, Host};

pub use crate::audio::pipeline::{
    take_host_lateness, LatencyReport, OutputHealth, SourceHealth, VirtualDevices, VirtualInput,
    VirtualSpeaker,
};

type Events = Arc<Mutex<Vec<(String, serde_json::Value)>>>;

/// A pipeline running on virtual devices, and what it told the UI.
pub struct Rig {
    pipeline: Option<ActivePipeline>,
    devices: Arc<VirtualDevices>,
    host: Host,
    events: Events,
}

impl Rig {
    pub fn new(devices: VirtualDevices) -> Self {
        let devices = Arc::new(devices);
        let events: Events = Arc::default();
        let sink = events.clone();
        let host = Host::with_virtual_devices(devices.clone(), move |event, payload| {
            sink.lock().unwrap().push((event.to_string(), payload));
        });
        Self {
            pipeline: None,
            devices,
            host,
            events,
        }
    }

    /// Starts `graph`, or moves the running pipeline to it, as the editor
    /// does. `graph` is the payload the frontend sends.
    pub fn apply(&mut self, graph: serde_json::Value) -> Result<(), String> {
        let spec: GraphSpec = serde_json::from_value(graph.clone()).map_err(|e| e.to_string())?;
        let valid = spec.validate().map_err(|e| e.to_string())?;
        let pipeline = self.pipeline.get_or_insert_with(ActivePipeline::new);
        pipeline
            .reconcile(&valid, self.host.clone())
            .map_err(|e| e.to_string())?;
        // A file's loop is not part of the graph: the editor sets it on the
        // running reader right after every start.
        for node in graph["nodes"].as_array().into_iter().flatten() {
            if node["kind"] == "audioFile" {
                if let (Some(id), Some(on)) =
                    (node["id"].as_str(), node["data"]["loopEnabled"].as_bool())
                {
                    pipeline.set_audio_file_loop(id, on);
                }
            }
        }
        Ok(())
    }

    /// Stops everything, as the stop button does.
    pub fn stop(&mut self) {
        self.pipeline = None;
    }

    /// What speaker `id` played, interleaved.
    pub fn played(&self, id: &str) -> Vec<f32> {
        self.devices.played(id)
    }

    pub fn clear_played(&self) {
        self.devices.clear_played();
    }

    pub fn health(&self) -> (Vec<SourceHealth>, Vec<OutputHealth>) {
        self.pipeline
            .as_ref()
            .map(ActivePipeline::health)
            .unwrap_or_default()
    }

    /// What the engine does between commands: rebuilds a speaker whose
    /// callbacks are larger than its graph renders at once.
    pub fn fit_speaker_callbacks(&mut self) -> Result<(), String> {
        match self.pipeline.as_mut() {
            Some(p) => p
                .fit_speaker_callbacks(self.host.clone())
                .map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// Engine blocks speaker node `id`'s graph renders per callback.
    pub fn speaker_burst(&self, id: &str) -> Option<usize> {
        self.pipeline.as_ref().and_then(|p| p.speaker_burst(id))
    }

    pub fn latency(&mut self) -> Option<LatencyReport> {
        self.pipeline.as_mut().map(ActivePipeline::latency_report)
    }

    /// Payloads of every `event` sent so far.
    pub fn events(&self, event: &str) -> Vec<serde_json::Value> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, _)| e == event)
            .map(|(_, p)| p.clone())
            .collect()
    }

    pub fn update_effect(&self, node_id: &str, data: serde_json::Value) {
        if let Some(p) = &self.pipeline {
            p.update_effect(node_id, &data);
        }
    }

    pub fn seek_file(&self, node_id: &str, frame: i64) {
        if let Some(p) = &self.pipeline {
            p.seek_audio_file(node_id, frame);
        }
    }

    pub fn set_file_paused(&self, node_id: &str, paused: bool) {
        if let Some(p) = &self.pipeline {
            p.set_audio_file_paused(node_id, paused);
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Writes `seconds` of a sine at `hz` to `path` in `format` (the JSON the
/// frontend stores for a recording format), with the app's own encoders.
pub fn write_tone_file(
    path: &Path,
    format: serde_json::Value,
    sample_rate: u32,
    channels: u16,
    seconds: f32,
    hz: f32,
    amplitude: f32,
) -> Result<(), String> {
    let format: RecordingFormat = serde_json::from_value(format).map_err(|e| e.to_string())?;
    let mut encoder =
        crate::audio::encoders::build_encoder(path, sample_rate, channels, format, false)
            .map_err(|e| e.to_string())?;
    let frames = (seconds * sample_rate as f32) as usize;
    let step = std::f32::consts::TAU * hz / sample_rate as f32;
    let samples: Vec<f32> = (0..frames)
        .flat_map(|f| std::iter::repeat_n(amplitude * (f as f32 * step).sin(), channels as usize))
        .collect();
    encoder
        .write_interleaved(&samples)
        .map_err(|e| e.to_string())?;
    encoder.finalize().map_err(|e| e.to_string())
}
