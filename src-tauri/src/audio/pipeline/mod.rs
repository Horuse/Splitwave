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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use rtrb::Producer;
use tracing::{info, warn};

use crate::audio::effects::{
    EffectControl, EffectRegistry, GrHandle, LufsHandle, MeterHandle, WaveformHandle,
};
use crate::audio::graph::{InputSpec, OutputSpec, RecordingFormat, ValidGraph};
use crate::audio::input_bridge::{broadcast_channel, BroadcastTx, CaptureStats, WriteClock};
use crate::error::{AppError, AppResult};

mod asrc;
mod cue;
mod cushion;
pub use cue::play as play_cue;
pub(crate) mod dag;
mod file_reader;
mod host;
pub use host::Host;
mod input;
mod latency;
pub use latency::LatencyReport;
mod meter;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native;
mod output;
pub(crate) use worker::RtThread;
mod sig;
mod virtual_io;
pub use virtual_io::{VirtualDevices, VirtualInput, VirtualSpeaker};
mod worker;

use dag::{build_output_graph, ring_capacity_frames, OutputGraph, OutputMeta, SourceMeta};
use file_reader::SeekFlush;
use input::{configure_io, resolve_input, start_input_stream, InputHandle, ResolvedInput};
use latency::{breakdown, DeviceIo, NodeTiming, PathInput, PathOutput};
use meter::{spawn_meter_thread, spawn_xrun_thread, MeterTickThread, XrunTickThread};
use output::{
    resolve_output, start_monitor_worker, start_recorder_worker, start_wire_sender_worker,
    RecorderWorker, ResolvedOutput, SpeakerIo, SpeakerStream,
};
use sig::{compute_output_sig, OutputSig, MONITOR_KEY};
use worker::WorkerCtrl;

/// A source's counters (see `dag::SourceStats`), in samples.
#[derive(Debug, Clone)]
pub struct SourceHealth {
    pub label: String,
    /// The output whose graph reads this source (`monitor` for the monitor).
    pub output_id: String,
    pub xrun: u64,
    pub stalled: u64,
    pub trimmed: u64,
    pub consumed: u64,
}

/// An output's worker: blocks rendered so far, and what a block is.
#[derive(Debug, Clone)]
pub struct OutputHealth {
    pub label: String,
    pub blocks: u64,
    pub block_frames: usize,
    pub sample_rate: u32,
}

/// Longest a hot swap waits for its fresh bridges to collect a block. Only an
/// input that delivers nothing (paused, or a quiet tap) waits this long.
const SWAP_PREFILL_MAX: std::time::Duration = std::time::Duration::from_millis(25);

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
    /// Latency and working block per effect node, for the report.
    node_timings: HashMap<String, NodeTiming>,
    /// What each output's running graph was built from, so the next graph
    /// swapped in on the same worker can carry its unchanged nodes over.
    carried_nodes: HashMap<String, HashMap<String, dag::CarriedNode>>,
    /// Bridges of outputs being swapped in place, by (output, input): a source
    /// carried over keeps the bridge that feeds its ring.
    swapping_slots: HashMap<(String, String), Vec<usize>>,
    /// Fill gauges of the network receive buffers each output reads.
    receive_buffers: HashMap<String, Vec<crate::audio::stream_recv::FillGauge>>,
    /// Each speaker's overload count at the last report.
    reported_overloads: HashMap<String, u64>,
    /// What each source measured of its delivery, for the source that
    /// replaces it.
    depth_memos: cushion::DepthMemos,
}

struct InputState {
    _handle: InputHandle,
    sample_rate: u32,
    channels: u32,
    /// The capture device's buffer and hardware latency, where known.
    io: Option<DeviceIo>,
    /// Frames the capture normalizer holds back, at the device's rate.
    normalizer_frames: u64,
    bridge_tx: BroadcastTx,
    bridges_by_output: HashMap<String, Vec<usize>>,
    /// Each bridge slot's delivery counters, for a source carried over into
    /// a new graph without a new slot.
    capture_by_slot: HashMap<usize, CaptureStats>,
    volume: Arc<AtomicU32>,
    paused: Option<Arc<AtomicBool>>,
    drain: Option<Arc<SeekFlush>>,
}

struct SpeakerState {
    /// Held only for its `Drop` -- cpal stream stop + worker join.
    _handle: SpeakerStream,
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
            node_timings: HashMap::new(),
            carried_nodes: HashMap::new(),
            swapping_slots: HashMap::new(),
            receive_buffers: HashMap::new(),
            reported_overloads: HashMap::new(),
            depth_memos: cushion::DepthMemos::default(),
        }
    }

    /// Diff `graph` against the running pipeline; only touch what changed.
    pub fn reconcile(&mut self, graph: &ValidGraph, host: Host) -> AppResult<()> {
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

        match self.apply_full(graph, host) {
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

    /// Queue a seek on the audio-file input identified by `node_id`. The
    /// reader flushes what is queued from the old position itself, at the
    /// moment it seeks (see `SeekFlush`). Silent no-op when the node isn't an
    /// AudioFile or the pipeline is stopped.
    pub fn seek_audio_file(&self, node_id: &str, frame: i64) {
        if let Some(state) = self.inputs.get(node_id) {
            if let Some(reader) = state._handle.audio_file_reader() {
                reader.seek_to().store(frame.max(0), Ordering::SeqCst);
                reader.wake();
            }
        }
    }

    /// Toggle loop-on-EOF for the audio-file input identified by `node_id`.
    /// Silent no-op when the node isn't an AudioFile or the pipeline is
    /// stopped.
    pub fn set_audio_file_loop(&self, node_id: &str, enabled: bool) {
        if let Some(state) = self.inputs.get(node_id) {
            if let Some(reader) = state._handle.audio_file_reader() {
                reader.loop_enabled().store(enabled, Ordering::SeqCst);
            }
        }
    }

    pub fn set_audio_file_paused(&self, node_id: &str, paused: bool) {
        if let Some(state) = self.inputs.get(node_id) {
            if let Some(p) = &state.paused {
                p.store(paused, Ordering::SeqCst);
            }
            if let Some(reader) = state._handle.audio_file_reader() {
                reader.wake();
            }
        }
    }

    /// Live volume update for an input node. Silent no-op when not running.
    pub fn set_input_volume(&self, node_id: &str, scalar: f32) {
        if let Some(state) = self.inputs.get(node_id) {
            state.volume.store(scalar.to_bits(), Ordering::Relaxed);
        }
    }

    /// Every source's and output's counters as they stand: what tests read
    /// to tell a pipeline that keeps up from one that drops or stalls.
    pub fn health(&self) -> (Vec<SourceHealth>, Vec<OutputHealth>) {
        let sources = self
            .source_stats
            .iter()
            .map(|s| SourceHealth {
                label: s.label.clone(),
                output_id: s.output_id.clone(),
                xrun: s.stats.xrun.load(Ordering::Relaxed),
                stalled: s.stats.stalled.load(Ordering::Relaxed),
                trimmed: s.stats.trimmed.load(Ordering::Relaxed),
                consumed: s.stats.consumed.load(Ordering::Relaxed),
            })
            .collect();
        let outputs = self
            .output_stats
            .iter()
            .map(|o| OutputHealth {
                label: o.label.clone(),
                blocks: o.blocks.load(Ordering::Relaxed),
                block_frames: o.block_frames,
                sample_rate: o.sample_rate,
            })
            .collect();
        (sources, outputs)
    }

    /// Round-trip latency of the slowest input-to-speaker path, the load of
    /// the busiest speaker callback since the last report, and every effect's
    /// timing. Zeroed when idle.
    pub fn latency_report(&mut self) -> LatencyReport {
        let buffer_frames = self.current.as_ref().map_or(0, |g| g.buffer_frames);
        let pipeline_rate = self.current.as_ref().map_or(48_000, |g| g.sample_rate);
        let mut report = LatencyReport {
            buffer_frames,
            sample_rate: self.current.as_ref().map_or(0, |g| g.sample_rate),
            nodes: self.node_timings.values().cloned().collect(),
            ..LatencyReport::default()
        };
        report.nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        for (id, s) in &self.speakers {
            let io = &s.io;
            let mut inputs: Vec<PathInput> = self
                .source_stats
                .iter()
                .filter(|m| m.output_id == *id)
                .map(|m| self.path_input(m))
                .collect();
            // A network input's latency is the receive buffer it plays from.
            for fill in self.receive_buffers.get(id).into_iter().flatten() {
                inputs.push(PathInput {
                    device: None,
                    queue_frames: fill.frames.load(Ordering::Relaxed) as u64,
                    normalizer_frames: 0,
                    rate: fill.rate.load(Ordering::Relaxed),
                });
            }
            let out = PathOutput {
                pipeline_rate,
                graph_latency_frames: io.graph_latency_frames.load(Ordering::Relaxed) as u64,
                device_rate: io.sample_rate,
                callback_frames: io.callback_frames.load(Ordering::Relaxed) as u64,
                carried_frames: io.carried_frames.load(Ordering::Relaxed) as u64,
                resampler_frames: io.resampler_delay_frames as u64,
                hardware_frames: io.hardware_frames,
            };
            let path = breakdown(&inputs, &out);
            if report
                .path
                .as_ref()
                .map_or(true, |p| path.total_ms > p.total_ms)
            {
                report.device_buffer_frames = Some(out.callback_frames as u32);
                report.path = Some(path);
            }
            let load = io.load_peak_permille.swap(0, Ordering::Relaxed) as f32 / 1000.0;
            report.dsp_load = report.dsp_load.max(load);
            let overloads = io.overloads.load(Ordering::Relaxed);
            let seen = self
                .reported_overloads
                .insert(id.clone(), overloads)
                .unwrap_or(0);
            report.overloads += overloads.saturating_sub(seen) as u32;
        }
        report
    }

    /// One source's share of a path. A ring-source reading a fan-out node
    /// carries on from the slowest source of the graph that owns the node,
    /// through that graph's delay compensation up to it, all counted in the
    /// ring's own rate.
    fn path_input(&self, m: &SourceMeta) -> PathInput {
        let input = m.input_id.as_ref().and_then(|i| self.inputs.get(i));
        // A file's queue is audio decoded ahead, not audio held back: nothing
        // live comes out later for it, and pause and seek drop it at once.
        let read_ahead = input.is_some_and(|i| i._handle.audio_file_reader().is_some());
        let own = PathInput {
            device: input.and_then(|i| i.io),
            queue_frames: if read_ahead {
                0
            } else {
                m.stats.queue_frames.load(Ordering::Relaxed)
            },
            normalizer_frames: input.map_or(0, |i| i.normalizer_frames),
            rate: m.native_sr,
        };
        let Some(up) = &m.upstream else {
            return own;
        };
        let in_rate = |frames: u64, rate: u32| frames * m.native_sr as u64 / rate.max(1) as u64;
        let before = self
            .source_stats
            .iter()
            .filter(|s| s.output_id == up.owner)
            .map(|s| self.path_input(s))
            .max_by_key(|p| in_rate(p.queue_frames + p.normalizer_frames, p.rate));
        PathInput {
            device: before.and_then(|b| b.device),
            queue_frames: own.queue_frames
                + up.frames as u64
                + before.map_or(0, |b| in_rate(b.queue_frames, b.rate)),
            normalizer_frames: before.map_or(0, |b| in_rate(b.normalizer_frames, b.rate)),
            rate: m.native_sr,
        }
    }

    fn teardown(&mut self) {
        if let Some(current) = &self.current {
            for inp in &current.inputs {
                if matches!(inp.spec, InputSpec::NetReceiver { .. }) {
                    crate::audio::netaudio::receiver::release(&inp.id);
                }
            }
            for out in &current.outputs {
                if matches!(out.spec, OutputSpec::NetSender { .. }) {
                    crate::audio::netaudio::sender::release(&out.id);
                }
            }
        }
        self.tear_down_outputs();
        self.stale_bridges.clear();
        self.carried_nodes.clear();
        self.swapping_slots.clear();
        self.receive_buffers.clear();
        self.inputs.clear();
        self.meters.clear();
        self.gr_handles.clear();
        self.scopes.clear();
    }

    /// Whether `out_id`'s new graph goes to the worker already running it,
    /// the only case in which the new graph can take over the old one's nodes.
    fn swaps_in_place(&self, out_id: &str, resolved: Option<&ResolvedOutput>) -> bool {
        match resolved {
            Some(ResolvedOutput::Speaker(spec)) => self
                .speakers
                .get(out_id)
                .is_some_and(|s| s.sample_rate == spec.sample_rate()),
            Some(ResolvedOutput::File { sample_rate, .. }) => self
                .recorders
                .get(out_id)
                .is_some_and(|r| r.sample_rate == *sample_rate),
            Some(ResolvedOutput::WireSender(_)) => self.wire_senders.contains_key(out_id),
            None => false,
        }
    }

    /// A source carried over keeps its ring, so the bridge feeding that ring
    /// stays instead of being retired with the rest of the output's.
    fn keep_carried_bridges(
        &mut self,
        out_id: &str,
        inputs: &[String],
        captured: &mut Vec<(String, String, CaptureStats)>,
    ) {
        for input_id in inputs {
            let key = (out_id.to_string(), input_id.clone());
            let Some(slots) = self.swapping_slots.remove(&key) else {
                continue;
            };
            self.stale_bridges
                .retain(|(id, slot)| !(id == input_id && slots.contains(slot)));
            let Some(state) = self.inputs.get_mut(input_id) else {
                continue;
            };
            for slot in &slots {
                if let Some(c) = state.capture_by_slot.get(slot) {
                    captured.push((input_id.clone(), out_id.to_string(), c.clone()));
                }
            }
            state
                .bridges_by_output
                .entry(out_id.to_string())
                .or_default()
                .extend(slots);
        }
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
        // Rate and buffer size are negotiated with every device when its
        // stream opens, so changing either reopens them all.
        let engine_format_changed = self
            .current
            .as_ref()
            .map_or(false, |c| !same_engine_format(c, new_graph));
        let mut cats: HashMap<String, Cat> = HashMap::new();
        for (id, new_sig) in &new_sigs {
            let cat = if engine_format_changed {
                Cat::Drop
            } else {
                match self.current_output_sig(id) {
                    Some(old) if old == new_sig => Cat::Full,
                    Some(old) if old.output_spec == new_sig.output_spec => Cat::GraphSwap,
                    _ => Cat::Drop,
                }
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

        // Before any output is dismantled: a tick landing mid-teardown would
        // count a speaker already gone against the old snapshot and report an
        // orphan stream and a stalled output that are neither.
        self.xrun_thread = None;

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
                    if swapping {
                        self.swapping_slots
                            .insert((id.clone(), input_id.clone()), slots.clone());
                    }
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
                crate::audio::netaudio::sender::release(id);
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
                if engine_format_changed {
                    return true;
                }
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
            if let Some(spec) = old_input_specs.get(id.as_str()) {
                if matches!(spec, InputSpec::NetReceiver { .. }) {
                    crate::audio::netaudio::receiver::release(&id);
                }
            }
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
        if !same_engine_format(current, graph) {
            return false;
        }
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

fn same_engine_format(a: &ValidGraph, b: &ValidGraph) -> bool {
    a.sample_rate == b.sample_rate && a.buffer_frames == b.buffer_frames
}

fn monitor_mode(graph: &ValidGraph) -> bool {
    graph.outputs.is_empty() || !dag::monitor_roots(graph).is_empty()
}

pub fn build(graph: &ValidGraph, host: Host) -> AppResult<ActivePipeline> {
    let mut p = ActivePipeline::new();
    p.reconcile(graph, host)?;
    Ok(p)
}

impl ActivePipeline {
    /// Surviving entries (left in place by `prepare_for_reconcile`) are
    /// reused; the rest are built fresh. On error `self` is in a half-built
    /// state -- the caller is responsible for calling `teardown`.
    fn apply_full(&mut self, graph: &ValidGraph, host: Host) -> AppResult<()> {
        let monitor_mode = monitor_mode(graph);
        let pipeline_sr = graph.sample_rate;

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
                let devices = host.virtual_devices();
                let resolved = match devices.and_then(|d| d.input_for(&inp.spec)) {
                    Some(device) => {
                        let (id, input) = device?;
                        ResolvedInput::Virtual { id, input }
                    }
                    #[cfg(any(target_os = "linux", target_os = "windows"))]
                    None => resolve_input(inp, pipeline_sr)?,
                    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
                    None => resolve_input(inp)?,
                };
                let sr = match &resolved {
                    ResolvedInput::AudioFile { sample_rate, .. } => *sample_rate,
                    _ => pipeline_sr,
                };
                input_native_sr.insert(inp.id.clone(), sr);
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
        let mut new_input_drain: HashMap<String, Arc<SeekFlush>> = HashMap::new();
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
                    new_input_drain.insert(inp.id.clone(), Arc::default());
                }
            }
        }
        let mut input_volumes: HashMap<String, Arc<AtomicU32>> = HashMap::new();
        let mut input_paused: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        let mut input_drain: HashMap<String, Arc<SeekFlush>> = HashMap::new();
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
                OutputSpec::FileRecording {
                    format: RecordingFormat::Aac { .. },
                    ..
                } => match pipeline_sr {
                    sr @ (32_000 | 44_100 | 48_000) => Some(sr),
                    _ => Some(48_000),
                },
                OutputSpec::FileRecording { .. } | OutputSpec::NetSender { .. } => {
                    Some(pipeline_sr)
                }
                _ => None,
            };
            let resolved = resolve_output(out, file_sr_hint, &host)?;
            output_runtime.insert(out.id.clone(), resolved);
        }

        // Tag each producer with its owning output_id so per-output
        // bridges can be tracked in `InputState.bridges_by_output`.
        let mut output_graphs: HashMap<String, OutputGraph> = HashMap::new();
        // Delivery counters of sources carried over, whose bridges are not
        // re-added below.
        let mut carried_captures: Vec<(String, String, CaptureStats)> = Vec::new();
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
        let mut pending_cuts: HashMap<String, HashMap<String, dag::CutLeaf>> = HashMap::new();
        for out in dag::owner_order(graph) {
            if !output_runtime.contains_key(&out.id) {
                continue;
            }
            let output_sr = match &out.spec {
                OutputSpec::Speaker { .. } => pipeline_sr,
                OutputSpec::FileRecording { .. }
                | OutputSpec::NetSender { .. }
                | OutputSpec::WebRtcSend { .. } => output_runtime
                    .get(&out.id)
                    .map(|o| o.sample_rate())
                    .unwrap_or(pipeline_sr),
            };
            // A speaker is paced by its device. A wire sender rides a timer,
            // but its latency is heard at the other end, so it runs the engine
            // block too. Recordings run large timer blocks nobody hears.
            let block_frames = match &out.spec {
                OutputSpec::Speaker { .. }
                | OutputSpec::NetSender { .. }
                | OutputSpec::WebRtcSend { .. } => graph.buffer_frames as usize,
                OutputSpec::FileRecording { .. } => dag::TIMER_BLOCK_FRAMES,
            };
            let mut my_pairs: Vec<(String, Producer<f32>)> = Vec::new();
            let cut_leaves = pending_cuts.remove(&out.id).unwrap_or_default();
            let previous = if self.swaps_in_place(&out.id, output_runtime.get(&out.id)) {
                self.carried_nodes.get(&out.id).cloned().unwrap_or_default()
            } else {
                HashMap::new()
            };
            let mut built = build_output_graph(
                Some(out.id.as_str()),
                output_sr,
                block_frames,
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
                &previous,
                &self.depth_memos,
            )?;
            self.keep_carried_bridges(&out.id, &built.carried_inputs, &mut carried_captures);
            self.carried_nodes
                .insert(out.id.clone(), std::mem::take(&mut built.carried));
            self.receive_buffers
                .insert(out.id.clone(), std::mem::take(&mut built.receive_buffers));
            // Wire publish taps for nodes this output owns and other outputs read.
            for (node, cons) in &cut_plan.consumers {
                if cons.is_empty()
                    || cut_plan.owner.get(node).map(String::as_str) != Some(out.id.as_str())
                {
                    continue;
                }
                let Some(&(idx, width, latency)) = built.node_meta.get(node) else {
                    continue;
                };
                for o2 in cons {
                    let (prod, consumer) =
                        rtrb::RingBuffer::<f32>::new(ring_capacity_frames(output_sr) * width);
                    let clock = Arc::new(WriteClock::default());
                    built.graph.attach_tap(idx, prod, clock.clone());
                    pending_cuts.entry(o2.clone()).or_default().insert(
                        node.clone(),
                        dag::CutLeaf {
                            consumer,
                            owner_sr: output_sr,
                            width,
                            clock,
                            upstream: dag::Upstream {
                                owner: out.id.clone(),
                                frames: latency,
                            },
                        },
                    );
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
            for t in built.node_timings {
                self.node_timings.insert(t.node_id.clone(), t);
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
                let monitor_sr = pipeline_sr;
                let mut my_pairs: Vec<(String, Producer<f32>)> = Vec::new();
                // Realtime: the monitor consumes live sources forever, so it must
                // drop backlog like any other live path. Without this its ring
                // grows unbounded whenever the DSP cannot keep up, and latency
                // climbs for as long as the pipeline runs.
                let previous = if self.monitor.is_some() {
                    self.carried_nodes
                        .get(MONITOR_KEY)
                        .cloned()
                        .unwrap_or_default()
                } else {
                    HashMap::new()
                };
                let mut built = build_output_graph(
                    None,
                    monitor_sr,
                    dag::TIMER_BLOCK_FRAMES,
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
                    &previous,
                    &self.depth_memos,
                )?;
                self.keep_carried_bridges(
                    MONITOR_KEY,
                    &built.carried_inputs,
                    &mut carried_captures,
                );
                self.carried_nodes
                    .insert(MONITOR_KEY.to_string(), std::mem::take(&mut built.carried));
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
                for t in built.node_timings {
                    self.node_timings.insert(t.node_id.clone(), t);
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
        let mut captured: Vec<(String, String, CaptureStats)> = carried_captures;
        // Bridges added beside a running one, and the samples that make one
        // engine block of their input.
        let mut fresh: Vec<(CaptureStats, u64)> = Vec::new();
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
                    state.capture_by_slot.insert(slot, capture.clone());
                    let block = output::device_block(
                        graph.buffer_frames as usize,
                        pipeline_sr,
                        state.sample_rate,
                    );
                    fresh.push((capture.clone(), block as u64 * state.channels as u64));
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
                let sample_rate = match &resolved {
                    ResolvedInput::AudioFile { sample_rate, .. } => *sample_rate,
                    _ => pipeline_sr,
                };
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
                let mut capture_by_slot = HashMap::new();
                for (out_id, prod) in tagged {
                    let (slot, capture) = bridge_tx.add(prod)?;
                    capture_by_slot.insert(slot, capture.clone());
                    captured.push((input_id.clone(), out_id.clone(), capture));
                    bridges_by_output.entry(out_id).or_default().push(slot);
                }
                let io = configure_io(&resolved, graph.buffer_frames as usize, pipeline_sr);
                let normalizer_frames =
                    input::normalizer_frames(&resolved, pipeline_sr, graph.buffer_frames as usize);
                let handle = start_input_stream(
                    &input_id,
                    resolved,
                    bridge_rx,
                    pipeline_sr,
                    paused.clone(),
                    drain.clone(),
                    None,
                    graph.buffer_frames as usize,
                    &host,
                )?;
                self.inputs.insert(
                    input_id,
                    InputState {
                        _handle: handle,
                        sample_rate,
                        channels,
                        io,
                        normalizer_frames,
                        bridge_tx,
                        bridges_by_output,
                        capture_by_slot,
                        volume,
                        paused,
                        drain,
                    },
                );
            }
        }

        for (input_id, state) in &self.inputs {
            let clock = state.bridge_tx.write_clock();
            for og in output_graphs.values_mut().chain(monitor_graph.as_mut()) {
                og.attach_write_clock(input_id, &clock);
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
            let sample_rate = pipeline_sr;
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
            let io = configure_io(&resolved, graph.buffer_frames as usize, pipeline_sr);
            let normalizer_frames =
                input::normalizer_frames(&resolved, pipeline_sr, graph.buffer_frames as usize);
            let handle = start_input_stream(
                &input_id,
                resolved,
                bridge_rx,
                pipeline_sr,
                paused.clone(),
                drain.clone(),
                Some(meter),
                graph.buffer_frames as usize,
                &host,
            )?;
            self.inputs.insert(
                input_id,
                InputState {
                    _handle: handle,
                    sample_rate,
                    channels,
                    io,
                    normalizer_frames,
                    bridge_tx,
                    bridges_by_output: HashMap::new(),
                    capture_by_slot: HashMap::new(),
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
            let deadline = std::time::Instant::now() + SWAP_PREFILL_MAX;
            while fresh
                .iter()
                .any(|(c, block)| c.fed.load(Ordering::Relaxed) < *block)
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
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
            let mut og = output_graphs.remove(&out.id).unwrap();
            let new_sig = compute_output_sig(graph, &out.id);
            match resolved {
                ResolvedOutput::Speaker(spec) => {
                    let out_channels = spec.out_channels();
                    og.set_out_channels(out_channels);
                    if let OutputSpec::Speaker { device_id } = &out.spec {
                        let locked: HashSet<String> = graph
                            .inputs
                            .iter()
                            .filter(|i| {
                                host.virtual_devices().is_none()
                                    && input::same_clock(&i.spec, device_id)
                            })
                            .map(|i| i.id.clone())
                            .collect();
                        if !locked.is_empty() {
                            info!(output = %out.id, ?locked, "inputs on the speaker's clock");
                        }
                        og.lock_inputs(&locked);
                    }
                    if let Some(state) = self.speakers.get_mut(&out.id) {
                        if state.sample_rate == spec.sample_rate() {
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
                    let sample_rate = spec.sample_rate();
                    let meter = MeterHandle::new(out.id.clone());
                    let (handle, ctrl, dead, io) = spec.open(&out.id, og, meter.clone(), &host)?;
                    // A new stream counts its overloads from zero.
                    self.reported_overloads.remove(&out.id);
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
                    let (worker, ctrl, wave) = start_recorder_worker(
                        out.id.clone(),
                        path,
                        sample_rate,
                        format,
                        channels,
                        append,
                        base_frames,
                        og,
                        host.clone(),
                    )?;
                    // Scope the recorder's waveform so the meter tick thread
                    // publishes it alongside the effect nodes' scopes.
                    self.scopes.insert(out.id.clone(), wave);
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
                ResolvedOutput::WireSender(_) => {
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

        self.swapping_slots.clear();
        // The swapped-in graphs own the live rings now; retire the ones that fed
        // their predecessors.
        for (input_id, slot) in std::mem::take(&mut self.stale_bridges) {
            if let Some(state) = self.inputs.get_mut(&input_id) {
                let _ = state.bridge_tx.remove(slot);
                state.bridge_tx.drain_discarded();
            }
        }

        self.node_timings
            .retain(|id, _| graph.effects.iter().any(|e| &e.id == id));

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
                host,
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
                self.speakers.len() as i64,
            ))
        };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::graph::{EdgeSpec, GraphSpec, NodeKind, NodeSpec, ValidGraph};
    use std::time::{Duration, Instant};

    fn node(id: &str, kind: NodeKind, data: serde_json::Value) -> NodeSpec {
        NodeSpec {
            id: id.to_string(),
            kind,
            data,
        }
    }

    fn mic(id: &str) -> NodeSpec {
        node(
            id,
            NodeKind::Microphone,
            serde_json::json!({ "deviceId": "dev" }),
        )
    }

    fn speaker(id: &str) -> NodeSpec {
        node(
            id,
            NodeKind::Speaker,
            serde_json::json!({ "deviceId": "dev" }),
        )
    }

    fn gain_node(id: &str, db: f32) -> NodeSpec {
        node(id, NodeKind::Gain, serde_json::json!({ "gainDb": db }))
    }

    fn edge(id: &str, from: &str, to: &str) -> EdgeSpec {
        EdgeSpec {
            id: id.to_string(),
            source: from.to_string(),
            source_handle: None,
            target: to.to_string(),
            target_handle: None,
        }
    }

    fn mic_to_speaker() -> ValidGraph {
        GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m"), gain_node("g", 0.0), speaker("s")],
            edges: vec![edge("e1", "m", "g"), edge("e2", "g", "s")],
        }
        .validate()
        .expect("valid")
    }

    #[test]
    fn monitor_mode_detection() {
        // No outputs at all → monitor (an output-less graph can only exist
        // internally, so it is assembled directly).
        let g = ValidGraph {
            inputs: Vec::new(),
            outputs: Vec::new(),
            effects: Vec::new(),
            edges: Vec::new(),
            sample_rate: 48_000,
            buffer_frames: 256,
        };
        assert!(monitor_mode(&g));

        // Analyzer present even with a real output → monitor for it.
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                speaker("s"),
                node("lm", NodeKind::LevelMeter, serde_json::json!({})),
            ],
            edges: vec![edge("e1", "m", "s"), edge("e2", "m", "lm")],
        }
        .validate()
        .expect("valid");
        assert!(monitor_mode(&g));

        // Plain graph → not monitor mode.
        assert!(!monitor_mode(&mic_to_speaker()));
    }

    #[test]
    fn empty_pipeline_is_a_quiescent_noop() {
        let mut p = ActivePipeline::new();
        let report = p.latency_report();
        assert_eq!(report.path, None, "idle pipeline reports no latency");
        assert_eq!(report.dsp_load, 0.0);
        // Every live-param command on an unknown node is a silent no-op.
        p.update_effect("ghost", &serde_json::json!({ "gainDb": -6.0 }));
        p.seek_audio_file("ghost", 100);
        p.set_audio_file_loop("ghost", true);
        p.set_audio_file_paused("ghost", false);
        p.set_input_volume("ghost", 0.5);
    }

    #[test]
    fn update_effect_routes_to_the_registered_control() {
        let mut p = ActivePipeline::new();
        let linear = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let bypass = Arc::new(AtomicBool::new(false));
        p.effect_controls.insert(
            "g".into(),
            EffectControl::Gain {
                linear: linear.clone(),
            },
        );
        p.effect_bypasses.insert("g".into(), bypass.clone());

        p.update_effect("g", &serde_json::json!({ "gainDb": -6.0 }));
        assert!(
            (f32::from_bits(linear.load(Ordering::Relaxed)) - 10f32.powf(-6.0 / 20.0)).abs() < 1e-6
        );

        p.update_effect("g", &serde_json::json!({ "bypassed": true }));
        assert!(bypass.load(Ordering::Relaxed));

        // Non-numeric plugin params are skipped, not fatal.
        p.update_effect("g", &serde_json::json!({ "pluginParams": { "nope": "x" } }));
    }

    #[test]
    fn is_structurally_current_requires_a_running_pipeline() {
        let p = ActivePipeline::new();
        assert!(!p.is_structurally_current(&mic_to_speaker()));
    }

    #[test]
    fn buffer_or_rate_change_reopens_every_stream() {
        let a = mic_to_speaker();
        assert!(same_engine_format(&a, &a.clone()));
        let mut b = a.clone();
        b.buffer_frames = 64;
        assert!(!same_engine_format(&a, &b), "buffer size change");
        let mut c = a.clone();
        c.sample_rate = 96_000;
        assert!(!same_engine_format(&a, &c), "sample rate change");
    }

    #[test]
    fn teardown_on_an_empty_pipeline_is_harmless() {
        let mut p = ActivePipeline::new();
        p.teardown();
        p.tear_down_outputs();
        assert!(p.inputs.is_empty());
        assert!(p.speakers.is_empty());
    }

    #[test]
    fn a_fan_out_path_counts_the_owner_graph_before_its_ring() {
        // Output b reads node g from a ring that output a publishes: its path
        // is a's slowest source, a's compensation up to g, then b's ring.
        let meta = |output: &str, queue: u64, upstream: Option<dag::Upstream>| {
            let stats = dag::SourceStats::new();
            stats.queue_frames.store(queue, Ordering::Relaxed);
            SourceMeta {
                label: String::new(),
                stats,
                channels: 2,
                native_sr: 48_000,
                frames_per_block: 64,
                input_id: None,
                output_id: output.into(),
                capture: None,
                upstream,
            }
        };
        let mut p = ActivePipeline::new();
        p.source_stats = vec![
            meta("a", 480, None),
            meta("a", 200, None),
            meta(
                "b",
                100,
                Some(dag::Upstream {
                    owner: "a".into(),
                    frames: 96,
                }),
            ),
        ];
        let path = p.path_input(&p.source_stats[2]);
        assert_eq!(path.queue_frames, 480 + 96 + 100);
    }

    #[test]
    fn a_carried_source_keeps_the_bridge_that_feeds_its_ring() {
        use crate::audio::pipeline::file_reader::file_reader_test_emitter::TestEmitter;

        let path = std::env::temp_dir().join(format!("pipeline_carry_{}.wav", std::process::id()));
        let mut enc = crate::audio::encoders::build_encoder(
            &path,
            48_000,
            2,
            crate::audio::graph::RecordingFormat::Wav {
                bit_depth: crate::audio::graph::WavBitDepth::F32,
            },
            false,
        )
        .unwrap();
        enc.write_interleaved(&[0.0; 64]).unwrap();
        enc.finalize().unwrap();
        let reader = file_reader::start_audio_file_reader(
            "f".into(),
            path.clone(),
            broadcast_channel().1,
            false,
            Arc::new(AtomicBool::new(true)),
            None,
            TestEmitter::default(),
        )
        .expect("reader");
        let (mut bridge_tx, _rx) = broadcast_channel();
        let (slot, capture) = bridge_tx.add(rtrb::RingBuffer::new(8).0).unwrap();
        let mut p = ActivePipeline::new();
        p.inputs.insert(
            "m".into(),
            InputState {
                _handle: InputHandle::AudioFile(reader),
                sample_rate: 48_000,
                channels: 2,
                io: None,
                normalizer_frames: 0,
                bridge_tx,
                bridges_by_output: HashMap::new(),
                capture_by_slot: HashMap::from([(slot, capture)]),
                volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
                paused: None,
                drain: None,
            },
        );
        // As `prepare_for_reconcile` leaves an output swapping in place.
        p.swapping_slots
            .insert(("s".into(), "m".into()), vec![slot]);
        p.stale_bridges.push(("m".into(), slot));

        let mut captured = Vec::new();
        p.keep_carried_bridges("s", &["m".to_string()], &mut captured);

        assert!(
            p.stale_bridges.is_empty(),
            "the carried ring's bridge is not retired"
        );
        assert_eq!(p.inputs["m"].bridges_by_output["s"], vec![slot]);
        assert_eq!(captured.len(), 1, "its delivery counters follow it");
        assert!(p.swapping_slots.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn carrying_one_output_leaves_the_others_bridges_to_retire() {
        use crate::audio::pipeline::file_reader::file_reader_test_emitter::TestEmitter;

        let path =
            std::env::temp_dir().join(format!("pipeline_carry_two_{}.wav", std::process::id()));
        let mut enc = crate::audio::encoders::build_encoder(
            &path,
            48_000,
            2,
            crate::audio::graph::RecordingFormat::Wav {
                bit_depth: crate::audio::graph::WavBitDepth::F32,
            },
            false,
        )
        .unwrap();
        enc.write_interleaved(&[0.0; 64]).unwrap();
        enc.finalize().unwrap();
        let reader = file_reader::start_audio_file_reader(
            "f".into(),
            path.clone(),
            broadcast_channel().1,
            false,
            Arc::new(AtomicBool::new(true)),
            None,
            TestEmitter::default(),
        )
        .expect("reader");
        let (mut bridge_tx, _rx) = broadcast_channel();
        let (kept, kept_capture) = bridge_tx.add(rtrb::RingBuffer::new(8).0).unwrap();
        let (retired, retired_capture) = bridge_tx.add(rtrb::RingBuffer::new(8).0).unwrap();
        let mut p = ActivePipeline::new();
        p.inputs.insert(
            "m".into(),
            InputState {
                _handle: InputHandle::AudioFile(reader),
                sample_rate: 48_000,
                channels: 2,
                io: None,
                normalizer_frames: 0,
                bridge_tx,
                bridges_by_output: HashMap::new(),
                capture_by_slot: HashMap::from([(kept, kept_capture), (retired, retired_capture)]),
                volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
                paused: None,
                drain: None,
            },
        );
        // Both outputs swap in place; only "s" carries its source over.
        p.swapping_slots
            .insert(("s".into(), "m".into()), vec![kept]);
        p.swapping_slots
            .insert(("r".into(), "m".into()), vec![retired]);
        p.stale_bridges.push(("m".into(), kept));
        p.stale_bridges.push(("m".into(), retired));

        let mut captured = Vec::new();
        p.keep_carried_bridges("s", &["m".to_string()], &mut captured);

        assert_eq!(p.stale_bridges, vec![("m".to_string(), retired)]);
        assert_eq!(p.inputs["m"].bridges_by_output["s"], vec![kept]);
        assert!(!p.inputs["m"].bridges_by_output.contains_key("r"));
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].1, "s", "stats follow the carried output only");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn file_commands_drive_the_audio_file_reader_atoms() {
        use crate::audio::pipeline::file_reader::file_reader_test_emitter::TestEmitter;

        let path = std::env::temp_dir().join(format!("pipeline_file_{}.wav", std::process::id()));
        let frames = 2_000usize;
        let mut block: Vec<f32> = (0..frames * 2).map(|i| (i % 9) as f32 / 9.0).collect();
        block.truncate(frames * 2);
        let mut enc = crate::audio::encoders::build_encoder(
            &path,
            48_000,
            2,
            crate::audio::graph::RecordingFormat::Wav {
                bit_depth: crate::audio::graph::WavBitDepth::F32,
            },
            false,
        )
        .unwrap();
        enc.write_interleaved(&block).unwrap();
        enc.finalize().unwrap();

        let paused = Arc::new(AtomicBool::new(true));
        let drain = Arc::new(SeekFlush::default());
        let volume = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let bridge = broadcast_channel().1;
        let emitter = TestEmitter::default();
        let events = emitter.events();
        let reader = file_reader::start_audio_file_reader(
            "f".into(),
            path.clone(),
            bridge,
            false,
            paused.clone(),
            Some(drain.clone()),
            emitter,
        )
        .expect("reader");
        let loop_enabled = reader.loop_enabled();
        let state = InputState {
            _handle: InputHandle::AudioFile(reader),
            sample_rate: 48_000,
            channels: 2,
            io: None,
            normalizer_frames: 0,
            bridge_tx: broadcast_channel().0,
            bridges_by_output: HashMap::new(),
            capture_by_slot: HashMap::new(),
            volume: volume.clone(),
            paused: Some(paused.clone()),
            drain: Some(drain.clone()),
        };
        let mut p = ActivePipeline::new();
        p.inputs.insert("f".to_string(), state);

        // Seek queues on the reader. A paused file has nothing queued, so the
        // reader moves without asking the graphs to let go of anything.
        p.seek_audio_file("f", 500);
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline
            && !events
                .lock()
                .unwrap()
                .iter()
                .any(|event| event["frames"] == 500 && event["paused"] == true)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|event| event["frames"] == 500 && event["paused"] == true),
            "reader did not apply the queued seek"
        );
        // Loop toggle lands on the reader.
        p.set_audio_file_loop("f", true);
        assert!(loop_enabled.load(Ordering::SeqCst));
        // Volume stores bits; paused flips the atom.
        p.set_input_volume("f", 0.5);
        assert_eq!(volume.load(Ordering::Relaxed), 0.5f32.to_bits());
        p.set_audio_file_paused("f", false);
        assert!(!paused.load(Ordering::SeqCst));
        // Unknown node ids stay silent.
        p.seek_audio_file("other", 5);
        p.set_input_volume("other", 0.1);
        drop(p);
        let _ = std::fs::remove_file(&path);
    }
}
