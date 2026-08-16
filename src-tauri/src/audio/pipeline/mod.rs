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

use rtrb::{Consumer, Producer};
use tauri::AppHandle;
use tracing::{info, warn};

use crate::audio::effects::{
    EffectControl, EffectRegistry, GrHandle, LufsHandle, MeterHandle, WaveformHandle,
};
use crate::audio::graph::{EffectSpec, InputSpec, OutputSpec, RecordingFormat, ValidGraph};
use crate::audio::input_bridge::{broadcast_channel, BroadcastTx, CaptureStats};
use crate::error::{AppError, AppResult};

mod cue;
pub use cue::play as play_cue;
pub(crate) mod dag;
mod file_reader;
mod input;
mod meter;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;
mod output;
#[cfg(target_os = "linux")]
pub(crate) use output::RtThread;
mod sig;
mod worker;

use dag::{
    build_output_graph, inputs_feeding_output, OutputGraph, OutputMeta, SourceMeta,
    RING_CAPACITY_FRAMES,
};
use input::{resolve_input, start_input_stream, InputHandle, ResolvedInput};
use meter::{spawn_meter_thread, spawn_xrun_thread, MeterTickThread, XrunTickThread};
use output::{
    resolve_output, start_monitor_worker, start_recorder_worker, start_speaker_stream,
    start_wire_sender_worker, RecorderWorker, ResolvedOutput, SpeakerHandle, SpeakerIo,
};
use sig::{compute_output_sig, OutputSig, MONITOR_KEY};
use worker::WorkerCtrl;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) const STATE_EVENT: &str = "audio://state";

/// Overlap between a hot-swapped output's old and new bridges: one DSP block at
/// 48 kHz plus slack, so the incoming sub-graph starts with its rings primed.
const SWAP_PREFILL: std::time::Duration = std::time::Duration::from_millis(25);

/// Long-lived audio runtime. Owns every cpal/SCK stream, every DspWorker
/// thread, the meter tick thread, and the effect parameter registry.
/// State is keyed by node id so `reconcile` can diff against `current` and
/// touch only what changed.
pub struct ActivePipeline {
    current: Option<ValidGraph>,

    inputs: HashMap<String, InputState>,
    speakers: HashMap<String, SpeakerState>,
    recorders: HashMap<String, RecorderState>,
    wire_senders: HashMap<String, WireSenderState>,
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
    /// Per-source and per-output rate/glitch stats, rebuilt each reconcile
    /// alongside the graphs.
    source_stats: Vec<SourceMeta>,
    output_stats: Vec<OutputMeta>,
    xrun_thread: Option<XrunTickThread>,
    /// `(input_id, slot)` bridges of hot-swapping outputs, kept feeding the old
    /// sub-graph while the new one's rings prefill. Removed after the swap.
    stale_bridges: Vec<(String, usize)>,
}

struct InputState {
    _handle: InputHandle,
    sample_rate: u32,
    channels: u32,
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
    // Output-tap level meter, re-registered into `meters` each reconcile so the
    // meter thread emits it. Persists across graph swaps (worker keeps running).
    meter: MeterHandle,
    // cpal callback counters (requested/read/callbacks); same stream survives a
    // GraphSwap, so the counters carry over instead of being rebuilt.
    io: SpeakerIo,
}

struct RecorderState {
    worker: RecorderWorker,
    #[allow(dead_code)]
    sample_rate: u32,
    sig: OutputSig,
    ctrl: WorkerCtrl,
}

struct WireSenderState {
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
            wire_senders: HashMap::new(),
            monitor: None,
            effect_registry: EffectRegistry::new(),
            effect_controls: HashMap::new(),
            effect_bypasses: HashMap::new(),
            meters: HashMap::new(),
            lufs: HashMap::new(),
            gr_handles: HashMap::new(),
            scopes: HashMap::new(),
            meter_thread: None,
            source_stats: Vec::new(),
            output_stats: Vec::new(),
            xrun_thread: None,
            stale_bridges: Vec::new(),
        }
    }

    /// Diff `graph` against the running pipeline; only touch what changed.
    pub fn reconcile(&mut self, graph: &ValidGraph, app: AppHandle) -> AppResult<()> {
        // Param-only resend: nothing structural changed, so leave every worker
        // (and the meter thread) running untouched.
        if self.is_structurally_current(graph) {
            self.current = Some(graph.clone());
            return Ok(());
        }

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
        // Some formats' editors only redraw when the host says a parameter
        // moved; doing it here keeps the notification (which locks) off the DSP
        // worker. Formats that carry the change to the plugin themselves report
        // it as unsupported, which is not a failure.
        if let Some(map) = data
            .get("pluginParams")
            .and_then(serde_json::Value::as_object)
        {
            if let Some(host) = crate::audio::plugins::registry::for_node(node_id) {
                for (id, value) in map {
                    let (Ok(id), Some(value)) = (id.parse::<u32>(), value.as_f64()) else {
                        continue;
                    };
                    let _ = host.notify_param_changed(node_id, id, value);
                }
            }
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

    /// Deepest speaker output latency in milliseconds, end to end: the input
    /// side's deepest source ring backlog, the graph's own lookahead (delay
    /// compensation aligned every path to it), and the adaptive output ring
    /// buffer. The ring tracks the device's buffer, so a large-buffer device
    /// runs at a higher (reported) latency than a low-latency one. Zero when
    /// idle.
    pub fn output_latency_ms(&self) -> u32 {
        self.speakers
            .iter()
            .map(|(id, s)| {
                let sr = s.sample_rate.max(1) as u64;
                let buffered = s.io.target_frames.load(Ordering::Relaxed).max(0) as u64;
                let out_ms = (buffered + s.io.graph_latency_frames as u64) * 1000 / sr;
                // Parallel inputs don't sum: the worst source is the path that
                // dominates the delay to this output.
                let in_ms = self
                    .source_stats
                    .iter()
                    .filter(|m| m.output_id == *id)
                    .map(|m| {
                        let frames =
                            m.stats.level.load(Ordering::Relaxed) / m.channels.max(1) as u64;
                        frames * 1000 / m.native_sr.max(1) as u64
                    })
                    .max()
                    .unwrap_or(0);
                (out_ms + in_ms) as u32
            })
            .max()
            .unwrap_or(0)
    }

    fn teardown(&mut self) {
        self.tear_down_outputs();
        self.stale_bridges.clear();
        self.inputs.clear();
        self.meters.clear();
        self.gr_handles.clear();
        self.scopes.clear();
    }

    // Signal all recorders before joining any so they cover the same wall-clock window.
    fn tear_down_outputs(&mut self) {
        // Before anything is dismantled: its next tick would measure a window
        // that straddles teardown and report the shortfall as an anomaly.
        self.xrun_thread = None;
        self.speakers.clear();
        for r in self.recorders.values() {
            r.worker.stop.store(true, Ordering::SeqCst);
        }
        for s in self.wire_senders.values() {
            s.worker.stop.store(true, Ordering::SeqCst);
        }
        if let Some(m) = &self.monitor {
            m.worker.stop.store(true, Ordering::SeqCst);
        }
        self.recorders.clear();
        self.wire_senders.clear();
        self.monitor = None;
        self.meter_thread = None;
        self.source_stats.clear();
        self.output_stats.clear();
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
        // A fan-out node is shared via a ring whose two ends must be rebuilt
        // together; if any cut participant is rebuilding, bump the Full ones to
        // GraphSwap so `apply_full` rebuilds them (and re-wires the ring) too.
        let participants =
            dag::plan_cuts(new_graph, monitor_mode.then_some(MONITOR_KEY)).participants();
        let group_dirty = participants
            .iter()
            .any(|id| !matches!(cats.get(id), Some(Cat::Full)));
        if group_dirty {
            for id in &participants {
                if let Some(cat @ Cat::Full) = cats.get_mut(id) {
                    *cat = Cat::GraphSwap;
                }
            }
        }

        let mut all_old: Vec<String> = Vec::new();
        all_old.extend(self.speakers.keys().cloned());
        all_old.extend(self.recorders.keys().cloned());
        all_old.extend(self.wire_senders.keys().cloned());
        if self.monitor.is_some() {
            all_old.push(MONITOR_KEY.to_string());
        }

        for id in &all_old {
            let cat = cats.get(id).copied().unwrap_or(Cat::Drop);
            if matches!(cat, Cat::Full) {
                continue;
            }
            // Surgically clear this output's bridges from each input. For
            // GraphSwap they stay live until the swap lands (`apply_full`
            // prefills the fresh rings first, so the old sub-graph plays on
            // instead of the new one starting from silence); for Drop the
            // worker goes away and bridges are gone with it.
            let swapping = matches!(cat, Cat::GraphSwap);
            for (input_id, state) in self.inputs.iter_mut() {
                if let Some(slots) = state.bridges_by_output.remove(id) {
                    for slot in slots {
                        if swapping {
                            self.stale_bridges.push((input_id.clone(), slot));
                        } else {
                            let _ = state.bridge_tx.remove(slot);
                        }
                    }
                }
            }
            if swapping {
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
            } else if let Some(state) = self.wire_senders.remove(id) {
                state.worker.stop.store(true, Ordering::SeqCst);
                drop(state);
            } else {
                self.speakers.remove(id);
            }
        }

        // Drop the meter / xrun tick threads -- they captured stale snapshots.
        // Fresh ones are spawned at the tail of `apply_full`.
        self.meter_thread = None;
        self.source_stats.clear();
        self.output_stats.clear();
        self.xrun_thread = None;

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
            .map(|g| g.inputs.iter().map(|i| (i.id.as_str(), &i.spec)).collect())
            .unwrap_or_default();
        let to_drop: Vec<String> = self
            .inputs
            .keys()
            .filter(|id| {
                match (
                    old_input_specs.get(id.as_str()),
                    new_input_specs.get(id.as_str()),
                ) {
                    (Some(o), Some(n)) if o == n => false,
                    _ => true,
                }
            })
            .cloned()
            .collect();
        for id in to_drop {
            self.inputs.remove(&id);
            self.meters.remove(&id);
        }

        Ok(())
    }

    /// True when `graph` differs from the running pipeline only in live params:
    /// every input spec, output key, and structural output sig is unchanged.
    /// Lets `reconcile` no-op a param-only resend without disturbing workers or
    /// the meter thread (params already flowed through `update_effect`).
    fn is_structurally_current(&self, graph: &ValidGraph) -> bool {
        let Some(current) = &self.current else {
            return false;
        };
        let cur_inputs: HashMap<&str, &InputSpec> = current
            .inputs
            .iter()
            .map(|i| (i.id.as_str(), &i.spec))
            .collect();
        let new_inputs: HashMap<&str, &InputSpec> = graph
            .inputs
            .iter()
            .map(|i| (i.id.as_str(), &i.spec))
            .collect();
        if cur_inputs != new_inputs {
            return false;
        }

        let mut new_keys: Vec<String> = graph.outputs.iter().map(|o| o.id.clone()).collect();
        if monitor_mode(graph) {
            new_keys.push(MONITOR_KEY.to_string());
        }
        let new_set: HashSet<String> = new_keys.iter().cloned().collect();

        let mut running: HashSet<String> = HashSet::new();
        running.extend(self.speakers.keys().cloned());
        running.extend(self.recorders.keys().cloned());
        running.extend(self.wire_senders.keys().cloned());
        if self.monitor.is_some() {
            running.insert(MONITOR_KEY.to_string());
        }
        if running != new_set {
            return false;
        }

        new_keys
            .iter()
            .all(|key| self.current_output_sig(key) == Some(&compute_output_sig(graph, key)))
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
        if let Some(s) = self.wire_senders.get(id) {
            return Some(&s.sig);
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
        matches!(
            e.spec,
            EffectSpec::LevelMeter(_)
                | EffectSpec::LufsMeter(_)
                | EffectSpec::Waveform(_)
                | EffectSpec::Spectrum(_)
        )
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
        let mut input_native_channels: HashMap<String, u32> = HashMap::new();
        let mut input_runtime: HashMap<String, ResolvedInput> = HashMap::new();
        for inp in &graph.inputs {
            // Network inputs have no capture device; they produce at the output
            // rate from their own socket, so they need no resolved input runtime.
            if matches!(
                inp.spec,
                InputSpec::NetReceiver { .. } | InputSpec::WebRtcRecv { .. }
            ) {
                continue;
            }
            if let Some(state) = self.inputs.get(&inp.id) {
                input_native_sr.insert(inp.id.clone(), state.sample_rate);
                input_native_channels.insert(inp.id.clone(), state.channels);
            } else {
                let resolved = resolve_input(inp)?;
                input_native_sr.insert(inp.id.clone(), resolved.sample_rate());
                input_native_channels.insert(inp.id.clone(), resolved.native_channels());
                input_runtime.insert(inp.id.clone(), resolved);
            }
        }

        // A Bluetooth device used as both Mic and Speaker gets forced into the
        // HFP profile (16/24 kHz mono), conflicting with the A2DP config we
        // resolved -- the OS picks one profile for the whole device.
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
                            "speaker device is also used as microphone -- the OS may force a reduced Bluetooth (HFP) profile"
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
                new_input_volumes.insert(
                    inp.id.clone(),
                    Arc::new(AtomicU32::new(inp.volume.to_bits())),
                );
                new_input_meters.insert(inp.id.clone(), MeterHandle::new(inp.id.clone()));
                if matches!(&inp.spec, InputSpec::AudioFile { .. }) {
                    new_input_paused
                        .insert(inp.id.clone(), Arc::new(AtomicBool::new(!inp.auto_start)));
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

        // Fan-out plan: nodes shared across outputs (and the monitor) are
        // computed once and read back via rings. When any participant rebuilds
        // they all must, so producer and consumer ends of every ring are
        // created in one pass.
        let cut_plan = dag::plan_cuts(graph, monitor_mode.then_some(MONITOR_KEY));
        let participants = cut_plan.participants();
        let base_changed =
            |id: &str| self.current_output_sig(id) != Some(&compute_output_sig(graph, id));
        let mut rebuild: HashSet<String> = HashSet::new();
        for out in &graph.outputs {
            if base_changed(&out.id) {
                rebuild.insert(out.id.clone());
            }
        }
        let group_dirty = participants.iter().any(|id| {
            if id == MONITOR_KEY {
                base_changed(MONITOR_KEY)
            } else {
                rebuild.contains(id)
            }
        });
        // The monitor rebuilds via its own `needs_build` below; force the real
        // outputs of a dirty cut group so every ring is re-wired atomically.
        let monitor_forced = group_dirty && participants.contains(MONITOR_KEY);
        if group_dirty {
            rebuild.extend(participants.iter().filter(|id| *id != MONITOR_KEY).cloned());
        }

        // Skip Full survivors; everything else needs a fresh sub-graph
        // (the new `OutputGraph` ships to GraphSwap workers via
        // `ctrl.send_graph`, or boots a new worker for Fresh starts).
        let mut output_runtime: HashMap<String, ResolvedOutput> = HashMap::new();
        for out in &graph.outputs {
            if !rebuild.contains(&out.id) {
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
        // `built.output`'s index in `self.output_stats`, by output id -- lets the
        // speaker-stream branch below fill in the real channel count and the
        // `SpeakerIo` handle once the cpal stream exists (both unknown when the
        // OutputMeta is first pushed here).
        let mut output_stat_idx: HashMap<String, usize> = HashMap::new();
        // A plugin feeding several outputs is built once per output; reset the
        // per-reconcile claim so exactly one build owns the editor instance.
        self.effect_registry.begin_reconcile();
        // Ring consumers stashed by an owner build, keyed by the consuming
        // output then node id; the consumer's build reads them as ring-sources.
        let mut pending_cuts: HashMap<String, HashMap<String, (Consumer<f32>, u32, usize)>> =
            HashMap::new();
        for out in &graph.outputs {
            if !output_runtime.contains_key(&out.id) {
                continue;
            }
            let output_sr = output_runtime
                .get(&out.id)
                .map(|o| o.sample_rate())
                .ok_or_else(|| AppError::Validation("missing output runtime".into()))?;
            let mut my_pairs: Vec<(String, Producer<f32>)> = Vec::new();
            let cut_leaves = pending_cuts.remove(&out.id).unwrap_or_default();
            let mut built = build_output_graph(
                Some(out.id.as_str()),
                output_sr,
                !matches!(out.spec, OutputSpec::FileRecording { .. }),
                graph,
                &input_native_sr,
                &input_native_channels,
                &mut my_pairs,
                &mut self.effect_registry,
                &input_volumes,
                &input_paused,
                &input_drain,
                &input_meters,
                cut_leaves,
            )?;
            // Wire publish taps for nodes this output owns and other outputs read.
            for (node, cons) in &cut_plan.consumers {
                if cons.is_empty()
                    || cut_plan.owner.get(node).map(String::as_str) != Some(out.id.as_str())
                {
                    continue;
                }
                let Some(&(idx, width)) = built.node_meta.get(node) else {
                    continue;
                };
                for o2 in cons {
                    let (prod, consumer) =
                        rtrb::RingBuffer::<f32>::new(RING_CAPACITY_FRAMES * width);
                    built.graph.attach_tap(idx, prod);
                    pending_cuts
                        .entry(o2.clone())
                        .or_default()
                        .insert(node.clone(), (consumer, output_sr, width));
                }
            }
            for (inp_id, prod) in my_pairs {
                all_pairs.push((out.id.clone(), inp_id, prod));
            }
            for (id, control) in built.controls {
                // Overwrite, not keep-first: a rebuilt node's control carries the
                // live handles/queue of the current instance; a stale entry would
                // route updates to a dropped instance.
                self.effect_controls.insert(id, control);
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
            self.source_stats.extend(built.sources);
            self.output_stats.push(built.output);
            output_stat_idx.insert(out.id.clone(), self.output_stats.len() - 1);
            output_graphs.insert(out.id.clone(), built.graph);
        }

        let mut monitor_graph: Option<OutputGraph> = None;
        if monitor_mode {
            let new_sig = compute_output_sig(graph, MONITOR_KEY);
            let needs_build =
                monitor_forced || self.monitor.as_ref().map_or(true, |m| m.sig != new_sig);
            if needs_build {
                let monitor_sr = input_native_sr.values().copied().max().unwrap_or(48_000);
                let mut my_pairs: Vec<(String, Producer<f32>)> = Vec::new();
                // Realtime: the monitor consumes live sources forever, so it must
                // drop backlog like any other live path. Without this its ring
                // grows unbounded whenever the DSP cannot keep up, and latency
                // climbs for as long as the pipeline runs.
                let built = build_output_graph(
                    None,
                    monitor_sr,
                    true,
                    graph,
                    &input_native_sr,
                    &input_native_channels,
                    &mut my_pairs,
                    &mut self.effect_registry,
                    &input_volumes,
                    &input_paused,
                    &input_drain,
                    &input_meters,
                    pending_cuts.remove(MONITOR_KEY).unwrap_or_default(),
                )?;
                for (inp_id, prod) in my_pairs {
                    all_pairs.push((MONITOR_KEY.to_string(), inp_id, prod));
                }
                for (id, control) in built.controls {
                    // Overwrite, not keep-first: a rebuilt node's control carries the
                    // live handles/queue of the current instance; a stale entry would
                    // route updates to a dropped instance.
                    self.effect_controls.insert(id, control);
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
                self.source_stats.extend(built.sources);
                self.output_stats.push(built.output);
                monitor_graph = Some(built.graph);
            }
        }

        let mut by_input: HashMap<String, Vec<(String, Producer<f32>)>> = HashMap::new();
        for (out_id, inp_id, prod) in all_pairs {
            by_input.entry(inp_id).or_default().push((out_id, prod));
        }

        let mut stale = std::mem::take(&mut self.stale_bridges);
        // (input_id, output_id, capture stats) for each slot added below, matched
        // into `self.source_stats` afterward -- SourceMeta is already built by
        // `build_output_graph` above, before the bridge slot (and its counters)
        // exists, so the two have to be joined here by their shared (input, output) key.
        let mut captured: Vec<(String, String, CaptureStats)> = Vec::new();
        for (input_id, tagged) in by_input {
            if self.inputs.contains_key(&input_id) {
                let state = self.inputs.get_mut(&input_id).unwrap();
                for (out_id, prod) in tagged {
                    // Overlapping bridges double this input's slot use until the
                    // swap lands; retire its stale ones early rather than fail
                    // the reconcile on an exhausted table.
                    if state.bridge_tx.free_slots() == 0 {
                        stale.retain(|(id, slot)| {
                            if id != &input_id {
                                return true;
                            }
                            let _ = state.bridge_tx.remove(*slot);
                            false
                        });
                        state.bridge_tx.drain_discarded();
                    }
                    let (slot, capture) = state.bridge_tx.add(prod)?;
                    captured.push((input_id.clone(), out_id.clone(), capture));
                    state
                        .bridges_by_output
                        .entry(out_id)
                        .or_default()
                        .push(slot);
                }
            } else {
                let resolved = input_runtime.remove(&input_id).ok_or_else(|| {
                    AppError::Validation(format!("input runtime missing for {input_id}"))
                })?;
                let sample_rate = resolved.sample_rate();
                let channels = resolved.native_channels();
                let meter = new_input_meters
                    .remove(&input_id)
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
                    let (slot, capture) = bridge_tx.add(prod)?;
                    captured.push((input_id.clone(), out_id.clone(), capture));
                    bridges_by_output.entry(out_id).or_default().push(slot);
                }
                let handle =
                    start_input_stream(&input_id, resolved, bridge_rx, paused.clone(), None, &app)?;
                self.inputs.insert(
                    input_id,
                    InputState {
                        _handle: handle,
                        sample_rate,
                        channels,
                        bridge_tx,
                        bridges_by_output,
                        volume,
                        paused,
                        drain,
                    },
                );
            }
        }

        for (input_id, out_id, capture) in captured {
            if let Some(meta) = self
                .source_stats
                .iter_mut()
                .find(|s| s.input_id.as_deref() == Some(input_id.as_str()) && s.output_id == out_id)
            {
                meta.capture = Some(capture);
            }
        }

        // Inputs that resolved but feed nothing: start their capture anyway so
        // the level meter runs. The capture meters directly (no DAG source).
        // File inputs are skipped -- don't auto-play an unrouted file.
        let unrouted: Vec<String> = input_runtime.keys().cloned().collect();
        for input_id in unrouted {
            if self.inputs.contains_key(&input_id) {
                continue;
            }
            let resolved = input_runtime.remove(&input_id).unwrap();
            if matches!(resolved, ResolvedInput::AudioFile { .. }) {
                continue;
            }
            let sample_rate = resolved.sample_rate();
            let channels = resolved.native_channels();
            let meter = new_input_meters
                .remove(&input_id)
                .unwrap_or_else(|| MeterHandle::new(input_id.clone()));
            self.meters.insert(input_id.clone(), meter.clone());
            let volume = new_input_volumes
                .remove(&input_id)
                .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
            let paused = new_input_paused.remove(&input_id);
            let drain = new_input_drain.remove(&input_id);
            let (bridge_tx, bridge_rx) = broadcast_channel();
            let handle = start_input_stream(
                &input_id,
                resolved,
                bridge_rx,
                paused.clone(),
                Some(meter),
                &app,
            )?;
            self.inputs.insert(
                input_id,
                InputState {
                    _handle: handle,
                    sample_rate,
                    channels,
                    bridge_tx,
                    bridges_by_output: HashMap::new(),
                    volume,
                    paused,
                    drain,
                },
            );
        }

        self.stale_bridges = stale;

        // Let the fresh rings collect a block before the swap: a worker handed a
        // sub-graph whose sources are empty emits zero-fill until the input
        // callback catches up, which is an audible dropout on every edit.
        if !self.stale_bridges.is_empty() {
            std::thread::sleep(SWAP_PREFILL);
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
            let mut og = output_graphs.remove(&out.id).unwrap();
            let new_sig = compute_output_sig(graph, &out.id);
            match resolved {
                ResolvedOutput::Speaker(spec) => {
                    let out_channels = spec.out_channels;
                    og.set_out_channels(out_channels);
                    if let Some(state) = self.speakers.get_mut(&out.id) {
                        if state.sample_rate == spec.sample_rate {
                            state.ctrl.send_graph(og)?;
                            state.sig = new_sig;
                            // Same cpal stream keeps running -- carry its
                            // counters into this reconcile's OutputMeta.
                            if let Some(&idx) = output_stat_idx.get(&out.id) {
                                self.output_stats[idx].channels = out_channels;
                                self.output_stats[idx].io = Some(state.io.clone());
                            }
                            continue;
                        }
                        // Sample rate changed (device reconfigured under us
                        // or a Bluetooth profile switch) -- can't swap, must
                        // restart the cpal stream. Drop the worker first.
                        self.speakers.remove(&out.id);
                    }
                    let sample_rate = spec.sample_rate;
                    let meter = MeterHandle::new(out.id.clone());
                    let (handle, ctrl, dead, io) =
                        start_speaker_stream(&out.id, spec, og, meter.clone(), &app)?;
                    if let Some(&idx) = output_stat_idx.get(&out.id) {
                        self.output_stats[idx].channels = out_channels;
                        self.output_stats[idx].io = Some(io.clone());
                    }
                    self.speakers.insert(
                        out.id.clone(),
                        SpeakerState {
                            _handle: handle,
                            sample_rate,
                            sig: new_sig,
                            ctrl,
                            dead,
                            meter,
                            io,
                        },
                    );
                }
                ResolvedOutput::File {
                    path,
                    sample_rate,
                    format,
                    channels,
                    append,
                    base_frames,
                } => {
                    og.set_out_channels(channels as usize);
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
                        channels,
                        append,
                        base_frames,
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
                ResolvedOutput::WireSender => {
                    let sample_rate = og.sample_rate();
                    if let Some(state) = self.wire_senders.get_mut(&out.id) {
                        state.ctrl.send_graph(og)?;
                        state.sig = new_sig;
                        continue;
                    }
                    let (worker, ctrl) = start_wire_sender_worker(og)?;
                    self.wire_senders.insert(
                        out.id.clone(),
                        WireSenderState {
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

        // The swapped-in graphs own the live rings now; retire the ones that fed
        // their predecessors.
        for (input_id, slot) in std::mem::take(&mut self.stale_bridges) {
            if let Some(state) = self.inputs.get_mut(&input_id) {
                let _ = state.bridge_tx.remove(slot);
                state.bridge_tx.drain_discarded();
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

        // Output-tap meters live on the speaker workers; surface them to the
        // meter thread alongside input/effect meters.
        for (id, s) in &self.speakers {
            self.meters.insert(id.clone(), s.meter.clone());
        }

        // Respawn the meter tick thread so it picks up new/changed
        // handles. The old thread (if any) was dropped by `teardown_*` /
        // `prepare_for_reconcile`.
        self.meter_thread = if self.meters.is_empty()
            && self.lufs.is_empty()
            && self.gr_handles.is_empty()
            && self.scopes.is_empty()
        {
            None
        } else {
            let meters_snapshot: Vec<MeterHandle> = self.meters.values().cloned().collect();
            let lufs_snapshot: Vec<LufsHandle> = self.lufs.values().cloned().collect();
            let gr_snapshot: Vec<GrHandle> = self.gr_handles.values().cloned().collect();
            let scopes_snapshot: Vec<WaveformHandle> = self.scopes.values().cloned().collect();
            Some(spawn_meter_thread(
                app,
                meters_snapshot,
                lufs_snapshot,
                gr_snapshot,
                scopes_snapshot,
            ))
        };

        self.xrun_thread = if self.source_stats.is_empty() && self.output_stats.is_empty() {
            None
        } else {
            Some(spawn_xrun_thread(
                self.source_stats.clone(),
                self.output_stats.clone(),
            ))
        };

        Ok(())
    }
}
