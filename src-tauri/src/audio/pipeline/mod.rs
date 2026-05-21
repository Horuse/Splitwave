//! Build and run a multi-input / multi-output audio pipeline as a DAG.
//!
//! Layout:
//! - Each input has one cpal/SCK callback that writes to N SPSC rings (one
//!   per output that consumes this input), at the input device's native SR.
//! - Each output owns an `OutputGraph` -- a topologically-sorted sub-DAG of
//!   sources + effects reachable backward from that output. A `DspWorker`
//!   thread mixes one block per real-time deadline and hands it off to:
//!     * Speaker: a stereo SPSC ring that the cpal output callback drains.
//!     * File: a `Box<dyn AudioEncoder>` (WAV / FLAC / ...).
//! - Effects with multiple incoming edges act as mixer-buses (sum first,
//!   then apply DSP). Effects are constrained to at most one outgoing edge
//!   in the validator.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use rtrb::Producer;
use tauri::AppHandle;
use tracing::{info, warn};

use crate::audio::device::DeviceKind;
use crate::audio::effects::{EffectControl, EffectRegistry, GrHandle, LufsHandle, MeterHandle, WaveformHandle};
use crate::audio::graph::{EffectSpec, InputSpec, OutputSpec, RecordingFormat, ValidGraph};
use crate::audio::input_bridge::{broadcast_channel, BroadcastTx};
use crate::error::{AppError, AppResult};

mod dag;
mod file_reader;
mod input;
mod meter;
mod output;
mod sig;
mod worker;

use dag::{build_output_graph, inputs_feeding_output, OutputGraph};
use input::{resolve_input, start_input_stream, InputHandle, ResolvedInput};
use meter::{spawn_meter_thread, MeterTickThread};
use output::{
    resolve_output, start_monitor_worker, start_recorder_worker, start_speaker_stream,
    RecorderWorker, ResolvedOutput, SpeakerHandle,
};
use sig::{compute_output_sig, OutputSig, MONITOR_KEY};
use worker::WorkerCtrl;

pub(super) const STATE_EVENT: &str = "audio://state";

/// Long-lived audio runtime. Owns every cpal/SCK stream, every DspWorker
/// thread, the meter tick thread, and the effect parameter registry.
/// State is keyed by node id so `reconcile` can diff against `current` and
/// touch only what changed.
pub struct ActivePipeline {
    current: Option<ValidGraph>,

    inputs: HashMap<String, InputState>,
    speakers: HashMap<String, SpeakerState>,
    recorders: HashMap<String, RecorderState>,
    /// Populated when there are no real outputs OR when monitor nodes are present.
    monitor: Option<MonitorState>,

    /// Persistent across reconciles so fan-out effects keep their atomics
    /// shared by node id.
    effect_registry: EffectRegistry,
    effect_controls: HashMap<String, EffectControl>,
    effect_bypasses: HashMap<String, Arc<AtomicBool>>,

    meters: HashMap<String, MeterHandle>,
    lufs: HashMap<String, LufsHandle>,
    gr_handles: HashMap<String, GrHandle>,
    scopes: HashMap<String, WaveformHandle>,
    meter_thread: Option<MeterTickThread>,
}

struct InputState {
    _handle: InputHandle,
    sample_rate: u32,
    bridge_tx: BroadcastTx,
    bridges_by_output: HashMap<String, Vec<usize>>,
    volume: Arc<AtomicU32>,
    paused: Option<Arc<AtomicBool>>,
    drain: Option<Arc<AtomicU64>>,
}

struct SpeakerState {
    /// Held only for its `Drop` -- cpal stream stop + worker join.
    _handle: SpeakerHandle,
    #[allow(dead_code)]
    sample_rate: u32,
    sig: OutputSig,
    ctrl: WorkerCtrl,
    dead: Arc<AtomicBool>,
}

struct RecorderState {
    worker: RecorderWorker,
    #[allow(dead_code)]
    sample_rate: u32,
    sig: OutputSig,
    ctrl: WorkerCtrl,
}

struct MonitorState {
    worker: RecorderWorker,
    sig: OutputSig,
    ctrl: WorkerCtrl,
}

impl ActivePipeline {
    /// Empty pipeline -- call `reconcile` to populate it from a `ValidGraph`.
    pub fn new() -> Self {
        Self {
            current: None,
            inputs: HashMap::new(),
            speakers: HashMap::new(),
            recorders: HashMap::new(),
            monitor: None,
            effect_registry: EffectRegistry::new(),
            effect_controls: HashMap::new(),
            effect_bypasses: HashMap::new(),
            meters: HashMap::new(),
            lufs: HashMap::new(),
            gr_handles: HashMap::new(),
            scopes: HashMap::new(),
            meter_thread: None,
        }
    }

    /// Diff `graph` against the running pipeline; only touch what changed.
    pub fn reconcile(&mut self, graph: &ValidGraph, app: AppHandle) -> AppResult<()> {
        for state in self.inputs.values_mut() {
            state.bridge_tx.drain_discarded();
        }

        if let Err(e) = self.prepare_for_reconcile(graph) {
            self.teardown();
            self.current = None;
            return Err(e);
        }

        // Dropped Consumers land in the discarded queue; drain before adding fresh Producers.
        for state in self.inputs.values_mut() {
            state.bridge_tx.drain_discarded();
        }

        match self.apply_full(graph, app) {
            Ok(()) => {
                self.current = Some(graph.clone());
                Ok(())
            }
            Err(e) => {
                self.teardown();
                self.current = None;
                Err(e)
            }
        }
    }

    pub fn update_effect(&self, node_id: &str, data: &serde_json::Value) {
        if let Some(control) = self.effect_controls.get(node_id) {
            control.apply_update(data);
        }
        if let Some(bypass) = self.effect_bypasses.get(node_id) {
            if let Some(b) = data.get("bypassed").and_then(serde_json::Value::as_bool) {
                bypass.store(b, Ordering::Relaxed);
            }
        }
    }

    /// Queue a seek on the audio-file input identified by `node_id`. Silent
    /// no-op when the node isn't an AudioFile or the pipeline is stopped.
    pub fn seek_audio_file(&self, node_id: &str, frame: i64) {
        if let Some(state) = self.inputs.get(node_id) {
            if let InputHandle::AudioFile(reader) = &state._handle {
                reader.seek_to().store(frame.max(0), Ordering::SeqCst);
            }
            if let Some(d) = &state.drain {
                d.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// Toggle loop-on-EOF for the audio-file input identified by `node_id`.
    /// Silent no-op when the node isn't an AudioFile or the pipeline is
    /// stopped.
    pub fn set_audio_file_loop(&self, node_id: &str, enabled: bool) {
        if let Some(state) = self.inputs.get(node_id) {
            if let InputHandle::AudioFile(reader) = &state._handle {
                reader.loop_enabled().store(enabled, Ordering::SeqCst);
            }
        }
    }

    pub fn set_audio_file_paused(&self, node_id: &str, paused: bool) {
        if let Some(state) = self.inputs.get(node_id) {
            if let Some(p) = &state.paused {
                p.store(paused, Ordering::SeqCst);
            }
        }
    }

    /// Live volume update for an input node. Silent no-op when not running.
    pub fn set_input_volume(&self, node_id: &str, scalar: f32) {
        if let Some(state) = self.inputs.get(node_id) {
            state.volume.store(scalar.to_bits(), Ordering::Relaxed);
        }
    }

    fn teardown(&mut self) {
        self.tear_down_outputs();
        self.inputs.clear();
        self.meters.clear();
        self.gr_handles.clear();
        self.scopes.clear();
    }

    // Signal all recorders before joining any so they cover the same wall-clock window.
    fn tear_down_outputs(&mut self) {
        self.speakers.clear();
        for r in self.recorders.values() {
            r.worker.stop.store(true, Ordering::SeqCst);
        }
        if let Some(m) = &self.monitor {
            m.worker.stop.store(true, Ordering::SeqCst);
        }
        self.recorders.clear();
        self.monitor = None;
        self.meter_thread = None;
        self.effect_controls.clear();
        self.effect_bypasses.clear();
        // Input meters live with their inputs and survive this teardown;
        // effect / output meters were dropped with the workers.
        let input_ids: HashSet<String> = self.inputs.keys().cloned().collect();
        self.meters.retain(|id, _| input_ids.contains(id));
        self.lufs.clear();
        self.gr_handles.clear();
        self.scopes.clear();
        self.effect_registry = EffectRegistry::new();
    }

    /// Classify each running output as Full (sig unchanged), GraphSwap (spec
    /// same, sub-graph differs -- hot-swap via ctrl.send_graph), or Drop
    /// (spec changed or removed). Tear down Drop outputs; Full survivors are
    /// untouched; GraphSwap outputs keep their cpal stream / recorder file open.
    fn prepare_for_reconcile(&mut self, new_graph: &ValidGraph) -> AppResult<()> {
        let monitor_mode = monitor_mode(new_graph);

        let mut new_sigs: HashMap<String, OutputSig> = HashMap::new();
        for out in &new_graph.outputs {
            new_sigs.insert(out.id.clone(), compute_output_sig(new_graph, &out.id));
        }
        if monitor_mode {
            new_sigs.insert(
                MONITOR_KEY.to_string(),
                compute_output_sig(new_graph, MONITOR_KEY),
            );
        }

        #[derive(Copy, Clone)]
        enum Cat {
            Full,
            GraphSwap,
            Drop,
        }
        let mut cats: HashMap<String, Cat> = HashMap::new();
        for (id, new_sig) in &new_sigs {
            let cat = match self.current_output_sig(id) {
                Some(old) if old == new_sig => Cat::Full,
                Some(old) if old.output_spec == new_sig.output_spec => Cat::GraphSwap,
                _ => Cat::Drop,
            };
            cats.insert(id.clone(), cat);
        }

        let mut all_old: Vec<String> = Vec::new();
        all_old.extend(self.speakers.keys().cloned());
        all_old.extend(self.recorders.keys().cloned());
        if self.monitor.is_some() {
            all_old.push(MONITOR_KEY.to_string());
        }

        for id in &all_old {
            let cat = cats.get(id).copied().unwrap_or(Cat::Drop);
            if matches!(cat, Cat::Full) {
                continue;
            }
            // Surgically clear this output's bridges from each input. For
            // GraphSwap, `apply_full` will route fresh ones; for Drop the
            // worker goes away and bridges are gone with it.
            for state in self.inputs.values_mut() {
                if let Some(slots) = state.bridges_by_output.remove(id) {
                    for slot in slots {
                        let _ = state.bridge_tx.remove(slot);
                    }
                }
            }
            if matches!(cat, Cat::GraphSwap) {
                continue;
            }
            if id == MONITOR_KEY {
                if let Some(m) = self.monitor.take() {
                    m.worker.stop.store(true, Ordering::SeqCst);
                    drop(m);
                }
            } else if let Some(state) = self.recorders.remove(id) {
                state.worker.stop.store(true, Ordering::SeqCst);
                drop(state);
            } else {
                self.speakers.remove(id);
            }
        }

        // Drop the meter tick thread -- it captured a stale snapshot. The
        // new one is spawned at the tail of `apply_full`.
        self.meter_thread = None;

        // Inputs whose spec changed (or vanished) drop here. Consumers
        // listed them in `OutputSig.inputs`, so spec change => sig change
        // => consumer was already classified `Drop` above; no surviving
        // output references stale input ids by this point.
        let new_input_specs: HashMap<&str, &InputSpec> = new_graph
            .inputs
            .iter()
            .map(|i| (i.id.as_str(), &i.spec))
            .collect();
        let old_input_specs: HashMap<&str, &InputSpec> = self
            .current
            .as_ref()
            .map(|g| {
                g.inputs
                    .iter()
                    .map(|i| (i.id.as_str(), &i.spec))
                    .collect()
            })
            .unwrap_or_default();
        let to_drop: Vec<String> = self
            .inputs
            .keys()
            .filter(|id| match (
                old_input_specs.get(id.as_str()),
                new_input_specs.get(id.as_str()),
            ) {
                (Some(o), Some(n)) if o == n => false,
                _ => true,
            })
            .cloned()
            .collect();
        for id in to_drop {
            self.inputs.remove(&id);
            self.meters.remove(&id);
        }

        Ok(())
    }

    fn current_output_sig(&self, id: &str) -> Option<&OutputSig> {
        if id == MONITOR_KEY {
            return self.monitor.as_ref().map(|m| &m.sig);
        }
        if let Some(s) = self.speakers.get(id) {
            if s.dead.load(Ordering::Relaxed) {
                return None;
            }
            return Some(&s.sig);
        }
        if let Some(r) = self.recorders.get(id) {
            return Some(&r.sig);
        }
        None
    }
}

impl Default for ActivePipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ActivePipeline {
    fn drop(&mut self) {
        self.teardown();
    }
}

fn monitor_mode(graph: &ValidGraph) -> bool {
    if graph.outputs.is_empty() {
        return true;
    }
    graph.effects.iter().any(|e| {
        matches!(e.spec, EffectSpec::LevelMeter(_) | EffectSpec::LufsMeter(_) | EffectSpec::Waveform(_))
    })
}

pub fn build(graph: &ValidGraph, app: AppHandle) -> AppResult<ActivePipeline> {
    let mut p = ActivePipeline::new();
    p.reconcile(graph, app)?;
    Ok(p)
}

impl ActivePipeline {
    /// Surviving entries (left in place by `prepare_for_reconcile`) are
    /// reused; the rest are built fresh. On error `self` is in a half-built
    /// state -- the caller is responsible for calling `teardown`.
    fn apply_full(&mut self, graph: &ValidGraph, app: AppHandle) -> AppResult<()> {
        let monitor_mode = monitor_mode(graph);

        let mut input_native_sr: HashMap<String, u32> = HashMap::new();
        let mut input_runtime: HashMap<String, ResolvedInput> = HashMap::new();
        for inp in &graph.inputs {
            if let Some(state) = self.inputs.get(&inp.id) {
                input_native_sr.insert(inp.id.clone(), state.sample_rate);
            } else {
                let resolved = resolve_input(inp)?;
                input_native_sr.insert(inp.id.clone(), resolved.sample_rate());
                input_runtime.insert(inp.id.clone(), resolved);
            }
        }

        // Bluetooth devices used as both Mic and Speaker get forced into HFP
        // (16/24 kHz mono), conflicting with the A2DP profile we resolved.
        {
            let mic_devices: HashSet<&str> = graph
                .inputs
                .iter()
                .filter_map(|i| match &i.spec {
                    InputSpec::Microphone { device_id } => Some(device_id.as_str()),
                    _ => None,
                })
                .collect();
            for out in &graph.outputs {
                if let OutputSpec::Speaker { device_id, .. } = &out.spec {
                    if mic_devices.contains(device_id.as_str()) {
                        warn!(
                            device = %device_id,
                            "speaker device is also used as microphone -- macOS will force HFP profile"
                        );
                    }
                }
            }
        }

        // Pre-create control atomics for new inputs so they can be wired into
        // the output DAG source nodes before InputState is constructed.
        let mut new_input_volumes: HashMap<String, Arc<AtomicU32>> = HashMap::new();
        let mut new_input_paused: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        let mut new_input_drain: HashMap<String, Arc<AtomicU64>> = HashMap::new();
        let mut new_input_meters: HashMap<String, MeterHandle> = HashMap::new();
        for inp in &graph.inputs {
            if !self.inputs.contains_key(&inp.id) {
                new_input_volumes.insert(inp.id.clone(), Arc::new(AtomicU32::new(inp.volume.to_bits())));
                new_input_meters.insert(inp.id.clone(), MeterHandle::new(inp.id.clone()));
                if matches!(&inp.spec, InputSpec::AudioFile { .. }) {
                    new_input_paused.insert(inp.id.clone(), Arc::new(AtomicBool::new(!inp.auto_start)));
                    new_input_drain.insert(inp.id.clone(), Arc::new(AtomicU64::new(0)));
                }
            }
        }
        let mut input_volumes: HashMap<String, Arc<AtomicU32>> = HashMap::new();
        let mut input_paused: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        let mut input_drain: HashMap<String, Arc<AtomicU64>> = HashMap::new();
        let mut input_meters: HashMap<String, MeterHandle> = HashMap::new();
        for (id, state) in &self.inputs {
            input_volumes.insert(id.clone(), state.volume.clone());
            if let Some(p) = &state.paused {
                input_paused.insert(id.clone(), p.clone());
            }
            if let Some(d) = &state.drain {
                input_drain.insert(id.clone(), d.clone());
            }
            if let Some(m) = self.meters.get(id) {
                input_meters.insert(id.clone(), m.clone());
            }
        }
        for (id, vol) in &new_input_volumes {
            input_volumes.insert(id.clone(), vol.clone());
        }
        for (id, p) in &new_input_paused {
            input_paused.insert(id.clone(), p.clone());
        }
        for (id, d) in &new_input_drain {
            input_drain.insert(id.clone(), d.clone());
        }
        for (id, m) in &new_input_meters {
            input_meters.insert(id.clone(), m.clone());
        }

        // Skip Full survivors; everything else needs a fresh sub-graph
        // (the new `OutputGraph` ships to GraphSwap workers via
        // `ctrl.send_graph`, or boots a new worker for Fresh starts).
        let mut output_runtime: HashMap<String, ResolvedOutput> = HashMap::new();
        for out in &graph.outputs {
            let new_sig = compute_output_sig(graph, &out.id);
            if self.current_output_sig(&out.id) == Some(&new_sig) {
                continue;
            }
            let file_sr_hint: Option<u32> = match &out.spec {
                OutputSpec::FileRecording {
                    format: RecordingFormat::Opus { .. } | RecordingFormat::Mp3 { .. },
                    ..
                } => Some(48_000),
                OutputSpec::FileRecording { .. } => inputs_feeding_output(out.id.as_str(), graph)
                    .into_iter()
                    .filter_map(|input_id| input_native_sr.get(input_id).copied())
                    .max(),
                _ => None,
            };
            let resolved = resolve_output(out, file_sr_hint)?;
            output_runtime.insert(out.id.clone(), resolved);
        }

        // Tag each producer with its owning output_id so per-output
        // bridges can be tracked in `InputState.bridges_by_output`.
        let mut output_graphs: HashMap<String, OutputGraph> = HashMap::new();
        let mut all_pairs: Vec<(String, String, Producer<f32>)> = Vec::new();
        for out in &graph.outputs {
            if !output_runtime.contains_key(&out.id) {
                continue;
            }
            let output_sr = output_runtime
                .get(&out.id)
                .map(|o| o.sample_rate())
                .ok_or_else(|| AppError::Validation("missing output runtime".into()))?;
            let mut my_pairs: Vec<(String, Producer<f32>)> = Vec::new();
            let built = build_output_graph(
                Some(out.id.as_str()),
                output_sr,
                matches!(out.spec, OutputSpec::Speaker { .. }),
                graph,
                &input_native_sr,
                &mut my_pairs,
                &mut self.effect_registry,
                &input_volumes,
                &input_paused,
                &input_drain,
                &input_meters,
            )?;
            for (inp_id, prod) in my_pairs {
                all_pairs.push((out.id.clone(), inp_id, prod));
            }
            for (id, control) in built.controls {
                self.effect_controls.entry(id).or_insert(control);
            }
            for (id, bypass) in built.bypasses {
                self.effect_bypasses.entry(id).or_insert(bypass);
            }
            for m in built.meters {
                self.meters.insert(m.node_id.clone(), m);
            }
            for l in built.lufs {
                self.lufs.insert(l.node_id.clone(), l);
            }
            for g in built.gr_handles {
                self.gr_handles.insert(g.node_id.clone(), g);
            }
            for s in built.scopes {
                self.scopes.insert(s.node_id.clone(), s);
            }
            output_graphs.insert(out.id.clone(), built.graph);
        }

        let mut monitor_graph: Option<OutputGraph> = None;
        if monitor_mode {
            let new_sig = compute_output_sig(graph, MONITOR_KEY);
            let needs_build = self
                .monitor
                .as_ref()
                .map_or(true, |m| m.sig != new_sig);
            if needs_build {
                let monitor_sr = input_native_sr.values().copied().max().unwrap_or(48_000);
                let mut my_pairs: Vec<(String, Producer<f32>)> = Vec::new();
                let built = build_output_graph(
                    None,
                    monitor_sr,
                    false,
                    graph,
                    &input_native_sr,
                    &mut my_pairs,
                    &mut self.effect_registry,
                    &input_volumes,
                    &input_paused,
                    &input_drain,
                    &input_meters,
                )?;
                for (inp_id, prod) in my_pairs {
                    all_pairs.push((MONITOR_KEY.to_string(), inp_id, prod));
                }
                for (id, control) in built.controls {
                    self.effect_controls.entry(id).or_insert(control);
                }
                for (id, bypass) in built.bypasses {
                    self.effect_bypasses.entry(id).or_insert(bypass);
                }
                for m in built.meters {
                    self.meters.insert(m.node_id.clone(), m);
                }
                for l in built.lufs {
                    self.lufs.insert(l.node_id.clone(), l);
                }
                for s in built.scopes {
                    self.scopes.insert(s.node_id.clone(), s);
                }
                monitor_graph = Some(built.graph);
            }
        }

        let mut by_input: HashMap<String, Vec<(String, Producer<f32>)>> = HashMap::new();
        for (out_id, inp_id, prod) in all_pairs {
            by_input.entry(inp_id).or_default().push((out_id, prod));
        }

        for (input_id, tagged) in by_input {
            if self.inputs.contains_key(&input_id) {
                let state = self.inputs.get_mut(&input_id).unwrap();
                for (out_id, prod) in tagged {
                    let slot = state.bridge_tx.add(prod)?;
                    state.bridges_by_output.entry(out_id).or_default().push(slot);
                }
            } else {
                let resolved = input_runtime.remove(&input_id).ok_or_else(|| {
                    AppError::Validation(format!("input runtime missing for {input_id}"))
                })?;
                let sample_rate = resolved.sample_rate();
                let meter = new_input_meters.remove(&input_id)
                    .unwrap_or_else(|| MeterHandle::new(input_id.clone()));
                self.meters.insert(input_id.clone(), meter);

                let volume = new_input_volumes
                    .remove(&input_id)
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                let paused = new_input_paused.remove(&input_id);
                let drain = new_input_drain.remove(&input_id);
                let (mut bridge_tx, bridge_rx) = broadcast_channel();
                let mut bridges_by_output: HashMap<String, Vec<usize>> = HashMap::new();
                for (out_id, prod) in tagged {
                    let slot = bridge_tx.add(prod)?;
                    bridges_by_output.entry(out_id).or_default().push(slot);
                }
                let handle = start_input_stream(&input_id, resolved, bridge_rx, paused.clone(), &app)?;
                self.inputs.insert(
                    input_id,
                    InputState {
                        _handle: handle,
                        sample_rate,
                        bridge_tx,
                        bridges_by_output,
                        volume,
                        paused,
                        drain,
                    },
                );
            }
        }

        // Hot-swap the new sub-graph into an existing worker when
        // `output_spec` is unchanged and the sample rate still matches;
        // otherwise stop the old worker and start fresh.
        for out in &graph.outputs {
            if !output_graphs.contains_key(&out.id) {
                continue;
            }
            let resolved = output_runtime.remove(&out.id).ok_or_else(|| {
                AppError::Validation(format!("output runtime missing for {}", out.id))
            })?;
            let og = output_graphs.remove(&out.id).unwrap();
            let new_sig = compute_output_sig(graph, &out.id);
            match resolved {
                ResolvedOutput::Speaker(spec) => {
                    if let Some(state) = self.speakers.get_mut(&out.id) {
                        if state.sample_rate == spec.sample_rate {
                            state.ctrl.send_graph(og)?;
                            state.sig = new_sig;
                            continue;
                        }
                        // Sample rate changed (device reconfigured under us
                        // or a Bluetooth profile switch) -- can't swap, must
                        // restart the cpal stream. Drop the worker first.
                        self.speakers.remove(&out.id);
                    }
                    let sample_rate = spec.sample_rate;
                    let (handle, ctrl, dead) = start_speaker_stream(&out.id, spec, og, &app)?;
                    self.speakers.insert(
                        out.id.clone(),
                        SpeakerState {
                            _handle: handle,
                            sample_rate,
                            sig: new_sig,
                            ctrl,
                            dead,
                        },
                    );
                }
                ResolvedOutput::File {
                    path,
                    sample_rate,
                    format,
                } => {
                    if let Some(state) = self.recorders.get_mut(&out.id) {
                        if state.sample_rate == sample_rate {
                            state.ctrl.send_graph(og)?;
                            state.sig = new_sig;
                            continue;
                        }
                        // SR change -- file format dictates a single SR per
                        // encoder lifetime, so we have to close and reopen.
                        let dropped = self.recorders.remove(&out.id).unwrap();
                        dropped.worker.stop.store(true, Ordering::SeqCst);
                        drop(dropped);
                    }
                    let (worker, ctrl) = start_recorder_worker(
                        out.id.clone(),
                        path,
                        sample_rate,
                        format,
                        og,
                        app.clone(),
                    )?;
                    self.recorders.insert(
                        out.id.clone(),
                        RecorderState {
                            worker,
                            sample_rate,
                            sig: new_sig,
                            ctrl,
                        },
                    );
                }
            }
        }
        if let Some(og) = monitor_graph {
            let new_sig = compute_output_sig(graph, MONITOR_KEY);
            if let Some(state) = self.monitor.as_mut() {
                state.ctrl.send_graph(og)?;
                state.sig = new_sig;
            } else {
                let (worker, ctrl) = start_monitor_worker(og)?;
                self.monitor = Some(MonitorState {
                    worker,
                    sig: new_sig,
                    ctrl,
                });
            }
        }

        // Sync volume atomics for all surviving inputs from the new graph spec.
        for inp in &graph.inputs {
            if let Some(state) = self.inputs.get(&inp.id) {
                state.volume.store(inp.volume.to_bits(), Ordering::Relaxed);
            }
        }

        info!(
            inputs = self.inputs.len(),
            speakers = self.speakers.len(),
            recorders = self.recorders.len(),
            outputs = graph.outputs.len(),
            effects = graph.effects.len(),
            edges = graph.edges.len(),
            "pipeline reconciled"
        );

        // Respawn the meter tick thread so it picks up new/changed
        // handles. The old thread (if any) was dropped by `teardown_*` /
        // `prepare_for_reconcile`.
        self.meter_thread = if self.meters.is_empty() && self.lufs.is_empty() && self.gr_handles.is_empty() && self.scopes.is_empty() {
            None
        } else {
            let meters_snapshot: Vec<MeterHandle> = self.meters.values().cloned().collect();
            let lufs_snapshot: Vec<LufsHandle> = self.lufs.values().cloned().collect();
            let gr_snapshot: Vec<GrHandle> = self.gr_handles.values().cloned().collect();
            let scopes_snapshot: Vec<WaveformHandle> = self.scopes.values().cloned().collect();
            Some(spawn_meter_thread(app, meters_snapshot, lufs_snapshot, gr_snapshot, scopes_snapshot))
        };

        Ok(())
    }
}

// ---------- native config resolution ----------
//
// We never ask cpal "what is this device's default/supported config?":
//   - `default_*_config` reads the *currently active* CoreAudio stream format,
//     which is absent for non-default routes (built-in speakers while AirPods
//     are connected) -> "Invalid property value".
//   - `supported_*_configs` reads `kAudioStreamPropertyAvailableVirtualFormats`,
//     which is also empty for those same non-default routes.
//
// AUHAL (cpal's underlying output unit on macOS) does NOT need to be told the
// device's "current" format up front -- it accepts whatever StreamConfig we
// hand it and asks CoreAudio to convert. So we read the device's nominal
// sample rate and channel count *directly* from CoreAudio HAL (which works
// regardless of routing state) and feed those into `build_*_stream`.
//
// Sample format is always `f32` -- the universal macOS audio type and the
// internal pipeline format.

pub(super) struct NativeConfig {
    pub config: cpal::StreamConfig,
    pub sample_format: cpal::SampleFormat,
    pub sample_rate: u32,
    pub channels: u16,
}

#[cfg(target_os = "macos")]
pub(super) fn native_config(
    kind: DeviceKind,
    _device: &cpal::Device,
    name: &str,
) -> AppResult<NativeConfig> {
    use crate::audio::macos_hal;
    let hal = match kind {
        DeviceKind::Input => macos_hal::find_input_device(name),
        DeviceKind::Output => macos_hal::find_output_device(name),
    }
    .ok_or_else(|| {
        AppError::Device(format!(
            "{kind:?} device {name:?} disappeared between enumeration and open"
        ))
    })?;

    let channels: u16 = hal
        .channels
        .try_into()
        .map_err(|_| AppError::Device(format!("device {name:?} has {} channels (too many)", hal.channels)))?;

    Ok(NativeConfig {
        config: cpal::StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(hal.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        },
        sample_format: cpal::SampleFormat::F32,
        sample_rate: hal.sample_rate,
        channels,
    })
}

#[cfg(not(target_os = "macos"))]
pub(super) fn native_config(
    kind: DeviceKind,
    device: &cpal::Device,
    name: &str,
) -> AppResult<NativeConfig> {
    // On Linux/Windows cpal's `supported_*_configs` is reliable for any device
    // the OS exposes -- no inactive-route quirk like macOS. Pick the range with
    // the highest max sample rate; force f32 sample format.
    let configs: Vec<cpal::SupportedStreamConfigRange> = match kind {
        DeviceKind::Input => device
            .supported_input_configs()
            .map_err(|e| AppError::Device(format!("query input configs for {name:?}: {e}")))?
            .collect(),
        DeviceKind::Output => device
            .supported_output_configs()
            .map_err(|e| AppError::Device(format!("query output configs for {name:?}: {e}")))?
            .collect(),
    };
    let best = configs
        .into_iter()
        .max_by_key(|c| c.max_sample_rate().0)
        .ok_or_else(|| AppError::Device(format!("device {name:?} exposes no configs")))?
        .with_max_sample_rate();
    Ok(NativeConfig {
        config: cpal::StreamConfig {
            channels: best.channels(),
            sample_rate: best.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        },
        sample_format: cpal::SampleFormat::F32,
        sample_rate: best.sample_rate().0,
        channels: best.channels(),
    })
}

