//! Builds graphs the way the editor sends them, runs them on virtual devices
//! through the app's own engine, and checks what came out.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use splitwave_lib::testkit::{
    OutputHealth, Rig, SourceHealth, VirtualDevices, VirtualInput, VirtualSpeaker,
};

pub const RATE: u32 = 48_000;
pub const TONE_HZ: f32 = 440.0;
pub const TONE_AMP: f32 = 0.25;

/// Scenarios run in real time; one at a time, so they never compete for the
/// CPU with each other on a small CI runner.
static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// Every device a scenario may name. Inputs play a 440 Hz tone.
pub fn devices() -> VirtualDevices {
    let d = VirtualDevices::default();
    let tone = |sample_rate, channels| VirtualInput {
        sample_rate,
        channels,
        tone_hz: TONE_HZ,
        amplitude: TONE_AMP,
    };
    d.add_input("mic", tone(RATE, 1));
    d.add_input("mic-44k", tone(44_100, 1));
    d.add_input("mic-96k", tone(96_000, 2));
    d.add_input("interface", tone(RATE, 4));
    d.add_input("system", tone(RATE, 2));
    d.add_input("app:com.example.player", tone(RATE, 2));
    let speaker = |sample_rate, channels| VirtualSpeaker {
        sample_rate,
        channels,
    };
    d.add_speaker("out", speaker(RATE, 2));
    d.add_speaker("out-2", speaker(RATE, 2));
    d.add_speaker("out-44k", speaker(44_100, 2));
    d.add_speaker("out-6ch", speaker(RATE, 6));
    d
}

/// A graph as the editor sends it.
pub struct Graph {
    nodes: Vec<Value>,
    edges: Vec<Value>,
    buffer: u32,
}

impl Graph {
    pub fn new(buffer: u32) -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            buffer,
        }
    }

    pub fn node(mut self, id: &str, kind: &str, data: Value) -> Self {
        self.nodes
            .push(json!({ "id": id, "kind": kind, "data": data }));
        self
    }

    pub fn edge(self, from: &str, to: &str) -> Self {
        self.edge_handles(from, None, to, None)
    }

    pub fn edge_handles(
        mut self,
        from: &str,
        source_handle: Option<&str>,
        to: &str,
        target_handle: Option<&str>,
    ) -> Self {
        let id = format!("e{}", self.edges.len());
        self.edges.push(json!({
            "id": id,
            "source": from,
            "sourceHandle": source_handle,
            "target": to,
            "targetHandle": target_handle,
        }));
        self
    }

    pub fn chain(mut self, ids: &[&str]) -> Self {
        for pair in ids.windows(2) {
            self = self.edge(pair[0], pair[1]);
        }
        self
    }

    pub fn json(&self) -> Value {
        json!({
            "nodes": self.nodes,
            "edges": self.edges,
            "sampleRate": RATE,
            "bufferFrames": self.buffer,
        })
    }
}

pub fn mic(device: &str) -> Value {
    json!({ "deviceId": device })
}

pub fn speaker(device: &str) -> Value {
    json!({ "deviceId": device })
}

pub fn audio_file(path: &std::path::Path) -> Value {
    json!({ "filePath": path.to_string_lossy(), "loopEnabled": true, "volume": 1.0, "autoStart": true })
}

/// Default data for every effect kind, as the editor creates them.
pub fn effect(kind: &str) -> Value {
    match kind {
        "gain" => json!({ "gainDb": 0 }),
        "mute" => json!({ "muted": false }),
        "channelBalance" => json!({ "leftGainDb": 0, "rightGainDb": 0 }),
        "saturator" => json!({ "thresholdDb": -0.3, "driveDb": 0 }),
        "eq" => json!({ "gainsDb": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] }),
        "levelMeter" => json!({}),
        "lufsMeter" => json!({ "target": -14 }),
        "waveform" => json!({ "segs": 4 }),
        "spectrum" => json!({ "smoothing": 0.5 }),
        "limiter" => json!({ "ceilingDb": -0.3, "lookaheadMs": 5, "releaseMs": 50 }),
        "compressor" => json!({ "thresholdDb": -18, "ratio": 3, "attackMs": 10,
                                "releaseMs": 100, "kneeDb": 6, "makeupDb": 0 }),
        "noiseGate" => json!({ "thresholdDb": -40, "rangeDb": -40, "attackMs": 1,
                               "holdMs": 50, "releaseMs": 200 }),
        "delay" => json!({ "timeMs": 250, "feedback": 0.4, "mix": 0.35 }),
        "reverb" => json!({ "roomSize": 0.5, "damping": 0.5, "width": 1, "mix": 0.33 }),
        "noiseSuppressor" => json!({ "attenuationLimitDb": 100, "postFilterBeta": 0,
                                     "minThreshDb": -10, "maxErbThreshDb": 30,
                                     "maxDfThreshDb": 20 }),
        "declick" => json!({ "sensitivity": 0.5, "maxWidthMs": 2 }),
        "deEsser" => json!({ "frequency": 6500, "thresholdDb": -30, "ratio": 4 }),
        other => panic!("no default data for {other}"),
    }
}

/// A fresh scratch path for this process.
pub fn temp_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("splitwave-scenarios-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

/// A looping test file: 2 s of the tone, stereo, in `format`.
pub fn tone_file(name: &str, format: Value, sample_rate: u32) -> PathBuf {
    let path = temp_path(name);
    if !path.exists() {
        splitwave_lib::testkit::write_tone_file(
            &path,
            format,
            sample_rate,
            2,
            2.0,
            TONE_HZ,
            TONE_AMP,
        )
        .expect("write tone file");
    }
    path
}

pub fn wav() -> Value {
    json!({ "kind": "wav", "bitDepth": "f32" })
}

pub fn flac() -> Value {
    json!({ "kind": "flac", "bitDepth": "i24", "compression": "default" })
}

pub fn mp3() -> Value {
    json!({ "kind": "mp3", "bitrateKbps": 192 })
}

/// What a scenario produced over its measured window.
pub struct Measured {
    pub played: HashMap<String, (Vec<f32>, usize)>,
    pub sources: Vec<(SourceHealth, SourceHealth)>,
    pub outputs: Vec<(OutputHealth, OutputHealth)>,
    pub window: Duration,
    pub rig: Rig,
}

pub const WARMUP: Duration = Duration::from_millis(500);
pub const WINDOW: Duration = Duration::from_millis(700);

/// Starts `graph`, lets it settle, then measures it for `WINDOW`.
pub fn run(graph: &Graph, speakers: &[(&str, usize)]) -> Measured {
    let mut rig = Rig::new(devices());
    rig.apply(graph.json()).expect("graph starts");
    measure(rig, speakers)
}

/// Measures an already running rig.
pub fn measure(rig: Rig, speakers: &[(&str, usize)]) -> Measured {
    std::thread::sleep(WARMUP);
    let (src_before, out_before) = rig.health();
    rig.clear_played();
    let started = Instant::now();
    std::thread::sleep(WINDOW);
    let window = started.elapsed();
    let (src_after, out_after) = rig.health();
    let played = speakers
        .iter()
        .map(|(id, ch)| (id.to_string(), (rig.played(id), *ch)))
        .collect();
    Measured {
        played,
        sources: pair(src_before, src_after, |s| {
            format!("{}@{}", s.label, s.output_id)
        }),
        outputs: pair(out_before, out_after, |o| o.label.clone()),
        window,
        rig,
    }
}

fn pair<T: Clone>(before: Vec<T>, after: Vec<T>, key: impl Fn(&T) -> String) -> Vec<(T, T)> {
    after
        .into_iter()
        .filter_map(|a| {
            before
                .iter()
                .find(|b| key(b) == key(&a))
                .map(|b| (b.clone(), a))
        })
        .collect()
}

impl Measured {
    /// Channel `ch` of speaker `id`.
    pub fn channel(&self, id: &str, ch: usize) -> Vec<f32> {
        let (samples, channels) = &self.played[id];
        samples
            .iter()
            .skip(ch)
            .step_by(*channels)
            .copied()
            .collect()
    }

    /// The speaker kept playing in real time, the tone never broke, and it
    /// came out at about `level` RMS.
    pub fn assert_tone(&self, id: &str, ch: usize, min_rms: f32) {
        let x = self.channel(id, ch);
        let expected = (self.window.as_secs_f64() * RATE as f64) as usize;
        assert!(
            x.len() as f64 > expected as f64 * 0.7,
            "{id}: played {} frames in {:?}",
            x.len(),
            self.window
        );
        assert!(x.iter().all(|s| s.is_finite()), "{id}: non-finite output");
        let mut run = 0;
        let mut worst = 0;
        for s in &x {
            if s.abs() < 1e-4 {
                run += 1;
                worst = worst.max(run);
            } else {
                run = 0;
            }
        }
        // A 440 Hz tone is never this quiet for a millisecond on end.
        assert!(
            worst < RATE as usize / 1000,
            "{id} ch{ch}: {worst} frames of silence"
        );
        let rms = (x.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
        assert!(
            rms as f32 >= min_rms,
            "{id} ch{ch}: RMS {rms:.4} below {min_rms}"
        );
    }

    /// Every worker, speakers and monitor alike, kept up with real time.
    pub fn assert_real_time(&self) {
        for (before, after) in &self.outputs {
            let frames = (after.blocks - before.blocks) as f64 * after.block_frames as f64;
            let rate = frames / self.window.as_secs_f64();
            let ratio = rate / after.sample_rate as f64;
            assert!(
                (0.85..1.15).contains(&ratio),
                "{} ran at {:.0}% of real time",
                after.label,
                ratio * 100.0
            );
        }
    }

    /// No source ran dry or stalled while measured.
    pub fn assert_no_dropouts(&self) {
        for (before, after) in &self.sources {
            let xrun = after.xrun - before.xrun;
            let stalled = after.stalled - before.stalled;
            assert!(
                xrun == 0 && stalled == 0,
                "{} in {}: {xrun} samples ran dry, {stalled} stalled",
                after.label,
                after.output_id
            );
        }
    }

    /// The everyday expectations: real time, no dropouts, the tone intact.
    pub fn assert_clean(&self, id: &str, min_rms: f32) {
        self.assert_real_time();
        self.assert_no_dropouts();
        self.assert_tone(id, 0, min_rms);
    }
}
