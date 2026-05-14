//! Build and run a multi-input / multi-output audio pipeline as a DAG.
//!
//! Layout:
//! - Each input has one cpal/SCK callback that writes to N SPSC rings (one
//!   per output that consumes this input), at the input device's native SR.
//! - Each output owns an `OutputGraph` — a topologically-sorted sub-DAG of
//!   sources + effects reachable backward from that output. A `DspWorker`
//!   thread mixes one block per real-time deadline and hands it off to:
//!     * Speaker: a stereo SPSC ring that the cpal output callback drains.
//!     * File: a crash-resistant `WavRecorder` (patches header on each flush).
//! - Effects with multiple incoming edges act as mixer-buses (sum first,
//!   then apply DSP). Effects are constrained to at most one outgoing edge
//!   in the validator.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::audio::clock::{ClockSource, SystemClockTicker};
use crate::audio::device::{self, DeviceKind};
use crate::audio::effects::{
    instantiate_effect, EffectControl, EffectRegistry, LufsHandle, MeterHandle, RuntimeEffect,
};
use crate::audio::graph::{
    InputSpec, OutputSpec, ValidGraph, ValidInput, ValidOutput,
};
use crate::audio::recorder::WavRecorder;
use crate::audio::resample::StereoResampler;
use crate::audio::streams;
use crate::error::{AppError, AppResult};

const STATE_EVENT: &str = "audio://state";
const METER_EVENT: &str = "audio://meter";
const LUFS_EVENT: &str = "audio://lufs";
const METER_TICK: Duration = Duration::from_millis(33);

/// Ring buffer length (in stereo f32 samples) per bridge. Sized for ~500 ms
/// of stereo audio at 96 kHz so the worker can ride out longer source pauses
/// (SCK silent gaps, scheduler hiccups) without overflowing the FAST source's
/// ring while waiting on a SLOW one.
const RING_CAPACITY: usize = 96_000;

/// Block size used by the resampler. 256 frames @ 48 kHz ≈ 5.3 ms.
const RESAMPLE_CHUNK: usize = 256;

/// Fallback sample rate for the file recorder when no input is connected to
/// it. With at least one input the recorder uses the highest connected input
/// rate to avoid lossy downsampling.
const RECORDER_DEFAULT_SR: u32 = 48_000;

pub struct ActivePipeline {
    _input_streams: Vec<InputHandle>,
    _speaker_streams: Vec<SpeakerHandle>,
    _workers: Vec<RecorderWorker>,
    /// node_id → live control handle. Each effect is 1→1 in the graph, so a
    /// node id corresponds to at most one runtime effect instance.
    effect_controls: HashMap<String, EffectControl>,
    /// LevelMeter tick thread — emits `audio://meter` events at ~30 Hz with
    /// peak + RMS per channel for every meter in the graph. `None` when no
    /// meters were placed.
    _meter_thread: Option<MeterTickThread>,
}

struct MeterTickThread {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for MeterTickThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl ActivePipeline {
    /// Apply a parameter patch from the frontend to the runtime effect.
    /// Silently no-ops when the node id isn't an effect in this pipeline —
    /// the frontend pushes updates for every node-data change and we don't
    /// want non-effect updates (mic device, file path) to surface as errors.
    pub fn update_effect(&self, node_id: &str, data: &serde_json::Value) {
        if let Some(control) = self.effect_controls.get(node_id) {
            control.apply_update(data);
        }
    }
}

/// Unified RAII handle for the different input source backends. The wrapped
/// value is held only for its `Drop` side-effect: the cpal stream stops on
/// drop, the SCK capture tears down the SCStream on drop.
#[allow(dead_code)]
enum InputHandle {
    Cpal(cpal::Stream),
    #[cfg(target_os = "macos")]
    Sck(crate::audio::sck_capture::SckCapture),
}

struct RecorderWorker {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for RecorderWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Fixed-capacity FIFO; allocates once. Overrun clamps and counts drops —
/// wrapping the write head past the read head would corrupt subsequent pops.
struct StagingRing {
    buf: Box<[f32]>,
    head: usize,
    tail: usize,
    len: usize,
    dropped: u64,
}

impl StagingRing {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: vec![0.0_f32; capacity].into_boxed_slice(),
            head: 0,
            tail: 0,
            len: 0,
            dropped: 0,
        }
    }

    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    #[allow(dead_code)]
    #[inline]
    fn dropped(&self) -> u64 {
        self.dropped
    }

    fn pop_into(&mut self, dst: &mut [f32]) -> usize {
        let n = dst.len().min(self.len);
        let cap = self.buf.len();
        for slot in dst.iter_mut().take(n) {
            *slot = self.buf[self.head];
            self.head = if self.head + 1 == cap { 0 } else { self.head + 1 };
        }
        self.len -= n;
        n
    }

    fn extend_from_slice(&mut self, src: &[f32]) {
        let cap = self.buf.len();
        let free = cap - self.len;
        debug_assert!(
            src.len() <= free,
            "StagingRing overrun: have {} + {} new > cap {}",
            self.len,
            src.len(),
            cap
        );
        let take = src.len().min(free);
        for &v in &src[..take] {
            self.buf[self.tail] = v;
            self.tail = if self.tail + 1 == cap { 0 } else { self.tail + 1 };
        }
        self.len += take;
        self.dropped = self.dropped.saturating_add((src.len() - take) as u64);
    }
}

/// One node in an output's DAG. `Source` reads from a ring + resamples,
/// `Effect` sums its upstreams' buffers and runs DSP. Both expose a stereo
/// `out_buf` of `DSP_BLOCK_FRAMES * 2` samples that downstream nodes consume.
enum DagNode {
    Source(SourceState),
    Effect(EffectState),
}

impl DagNode {
    fn out_buf(&self) -> &[f32] {
        match self {
            DagNode::Source(s) => &s.out_buf,
            DagNode::Effect(e) => &e.out_buf,
        }
    }
}

struct SourceState {
    /// Human-friendly id used only in the one-shot startup log.
    label: String,
    consumer: Consumer<f32>,
    /// `None` when input SR == output SR.
    resampler: Option<StereoResampler>,
    /// Samples pulled from the ring waiting to feed the resampler.
    input_staging: Vec<f32>,
    /// Resampled samples waiting to be drained into `out_buf`.
    out_pending: StagingRing,
    /// Per-block scratch — receives one resampler chunk before going into staging.
    chunk_tmp: Vec<f32>,
    /// Current block (length = DSP_BLOCK_FRAMES * 2).
    out_buf: Vec<f32>,
    /// Input-side (raw input-SR, stereo) sample count needed to produce one
    /// full output block. Used by the availability-pacing worker to know when
    /// this source has enough buffered audio to safely contribute a block.
    input_samples_per_block: usize,
    /// Wall-time of the most recent successful pop. Used to detect "silent"
    /// sources — if no samples have flowed for `STALL_THRESHOLD`, the worker
    /// stops waiting on this source so the OTHER sources don't end up with
    /// their rings overflowing while we hang on a dead one.
    last_pop_at: Instant,
    /// One-shot startup log so we can confirm samples actually start flowing.
    first_data_logged: bool,
}

/// How long a source can go without delivering before the availability-paced
/// worker stops waiting on it. SCK in normal operation delivers every ~20 ms,
/// so 150 ms is ~7× headroom — enough to avoid false positives on bursty
/// delivery, short enough that a real stall doesn't drown the FAST source's
/// ring buffer.
const STALL_THRESHOLD: Duration = Duration::from_millis(150);

impl SourceState {
    fn is_stalled(&self) -> bool {
        self.last_pop_at.elapsed() > STALL_THRESHOLD
    }

    /// True when the source can fill one output block without underrun, OR
    /// when it's been silent long enough that we should stop waiting on it.
    /// A stalled source contributes silence to the mix (fill_block zero-fills
    /// the part it can't supply).
    fn is_ready_for_block(&self) -> bool {
        if self.is_stalled() {
            return true;
        }
        let have = self.consumer.slots() + self.input_staging.len();
        have >= self.input_samples_per_block
    }
}

impl SourceState {
    fn fill_block(&mut self) {
        let need = self.out_buf.len();
        let mut written = self.out_pending.pop_into(&mut self.out_buf[..]);
        while written < need {
            self.try_refill_one_chunk();
            if self.out_pending.len() == 0 {
                // Ring empty too — zero-fill the rest (real underrun).
                for s in &mut self.out_buf[written..] {
                    *s = 0.0;
                }
                return;
            }
            let n = self.out_pending.pop_into(&mut self.out_buf[written..]);
            written += n;
        }
    }

    fn try_refill_one_chunk(&mut self) {
        if let Some(rs) = &mut self.resampler {
            let needed = rs.chunk_in() * 2;
            // Bulk read what we still need (one rtrb reservation instead of
            // one atomic op per sample — RT-friendly).
            let want = needed - self.input_staging.len();
            let avail = self.consumer.slots().min(want);
            if avail > 0 {
                if let Ok(chunk) = self.consumer.read_chunk(avail) {
                    let (first, second) = chunk.as_slices();
                    self.input_staging.extend_from_slice(first);
                    self.input_staging.extend_from_slice(second);
                    chunk.commit_all();
                    self.last_pop_at = Instant::now();
                }
            }
            if self.input_staging.len() < needed {
                return; // wait for more data on next call
            }
            self.chunk_tmp.clear();
            if let Err(e) =
                rs.process_chunk(&self.input_staging[..needed], &mut self.chunk_tmp)
            {
                warn!(source = %self.label, error = %e, "resampler chunk failed");
                self.input_staging.drain(..needed);
                return;
            }
            self.input_staging.drain(..needed);
        } else {
            self.chunk_tmp.clear();
            let want = RESAMPLE_CHUNK * 2;
            let avail = self.consumer.slots().min(want);
            if avail > 0 {
                if let Ok(chunk) = self.consumer.read_chunk(avail) {
                    let (first, second) = chunk.as_slices();
                    self.chunk_tmp.extend_from_slice(first);
                    self.chunk_tmp.extend_from_slice(second);
                    chunk.commit_all();
                    self.last_pop_at = Instant::now();
                }
            }
        }
        // Even-count guarantee (don't half a frame).
        let frames = self.chunk_tmp.len() / 2;
        self.chunk_tmp.truncate(frames * 2);
        if !self.chunk_tmp.is_empty() {
            if !self.first_data_logged {
                info!(source = %self.label, "source online");
                self.first_data_logged = true;
            }
            self.out_pending.extend_from_slice(&self.chunk_tmp);
        }
    }
}

struct EffectState {
    effect: RuntimeEffect,
    /// Each entry sums one upstream `out_buf` into this effect's `out_buf`.
    /// Topo sort guarantees every `src_idx` is < this effect's own index.
    incoming: Vec<IncomingEdge>,
    out_buf: Vec<f32>,
}

/// `delay` is `Some` when this path is shorter than the longest reaching the
/// same mixing point — pads it for sample-alignment before summing.
struct IncomingEdge {
    src_idx: usize,
    delay: Option<DelayLine>,
}

struct TerminalEdge {
    src_idx: usize,
    delay: Option<DelayLine>,
}

struct DelayLine {
    buf: Box<[f32]>,
    pos: usize,
}

impl DelayLine {
    fn new(delay_frames: usize) -> Self {
        Self {
            buf: vec![0.0; delay_frames * 2].into_boxed_slice(),
            pos: 0,
        }
    }

    fn process_and_add(&mut self, input: &[f32], dst: &mut [f32]) {
        let cap = self.buf.len();
        if cap == 0 {
            for (d, &v) in dst.iter_mut().zip(input.iter()) {
                *d += v;
            }
            return;
        }
        let mut pos = self.pos;
        for (i, &v) in input.iter().enumerate() {
            let delayed = self.buf[pos];
            self.buf[pos] = v;
            dst[i] += delayed;
            pos = if pos + 1 == cap { 0 } else { pos + 1 };
        }
        self.pos = pos;
    }
}

/// Per-output DAG runtime: sources + effects in topological order plus the
/// terminal edges whose buffers get summed into the final output.
struct OutputGraph {
    /// Reserved for drift-correction; not yet consumed.
    #[allow(dead_code)]
    sample_rate: u32,
    nodes: Vec<DagNode>,
    terminals: Vec<TerminalEdge>,
}

impl OutputGraph {
    /// True if every source has enough buffered input to produce one full
    /// output block without underrun. Availability-paced workers use this to
    /// gate block production.
    fn all_sources_ready(&self) -> bool {
        for node in &self.nodes {
            if let DagNode::Source(s) = node {
                if !s.is_ready_for_block() {
                    return false;
                }
            }
        }
        true
    }

    /// Fill `output` (must be `DSP_BLOCK_FRAMES * 2` long) with one block of
    /// mixed audio at `sample_rate`.
    fn process_block(&mut self, output: &mut [f32]) {
        for node in &mut self.nodes {
            if let DagNode::Source(s) = node {
                s.fill_block();
            }
        }
        // `split_at_mut` gives mutable access to effect `i` while keeping
        // immutable access to its upstreams (all at indices < i by topo sort).
        for i in 0..self.nodes.len() {
            let (head, tail) = self.nodes.split_at_mut(i);
            if let DagNode::Effect(eff) = &mut tail[0] {
                for s in eff.out_buf.iter_mut() {
                    *s = 0.0;
                }
                for edge in &mut eff.incoming {
                    let src = head[edge.src_idx].out_buf();
                    match &mut edge.delay {
                        Some(d) => d.process_and_add(src, &mut eff.out_buf),
                        None => {
                            for (dst, sv) in eff.out_buf.iter_mut().zip(src.iter()) {
                                *dst += *sv;
                            }
                        }
                    }
                }
                eff.effect.process(&mut eff.out_buf, DSP_BLOCK_FRAMES);
            }
        }
        for s in output.iter_mut() {
            *s = 0.0;
        }
        for terminal in &mut self.terminals {
            let src = self.nodes[terminal.src_idx].out_buf();
            match &mut terminal.delay {
                Some(d) => d.process_and_add(src, output),
                None => {
                    for (dst, sv) in output.iter_mut().zip(src.iter()) {
                        *dst += *sv;
                    }
                }
            }
        }
    }
}

/// Build everything from a validated graph and start playing.
pub fn build(graph: &ValidGraph, app: AppHandle) -> AppResult<ActivePipeline> {
    let monitor_mode = graph.outputs.is_empty();

    let mut input_native_sr: HashMap<String, u32> = HashMap::new();
    let mut input_runtime: HashMap<String, ResolvedInput> = HashMap::new();
    for inp in &graph.inputs {
        let resolved = resolve_input(inp)?;
        input_native_sr.insert(inp.id.clone(), resolved.sample_rate());
        input_runtime.insert(inp.id.clone(), resolved);
    }

    // Bluetooth devices used as both Mic and Speaker get forced into HFP
    // (16/24 kHz mono), conflicting with the A2DP profile we resolved.
    {
        use std::collections::HashSet;
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
                        "speaker device is also used as microphone — macOS will force HFP profile"
                    );
                }
            }
        }
    }

    // File recorders use the highest source SR feeding them so the WAV never
    // carries a downsampled signal.
    let mut output_runtime: HashMap<String, ResolvedOutput> = HashMap::new();
    for out in &graph.outputs {
        let file_sr_hint: Option<u32> = if matches!(&out.spec, OutputSpec::FileRecording { .. }) {
            inputs_feeding_output(out.id.as_str(), graph)
                .into_iter()
                .filter_map(|input_id| input_native_sr.get(input_id).copied())
                .max()
        } else {
            None
        };
        let resolved = resolve_output(out, file_sr_hint)?;
        output_runtime.insert(out.id.clone(), resolved);
    }

    let mut producers_by_input: HashMap<String, Vec<Producer<f32>>> = HashMap::new();
    let mut output_graphs: HashMap<String, OutputGraph> = HashMap::new();
    let mut effect_controls: HashMap<String, EffectControl> = HashMap::new();
    let mut all_meters: Vec<MeterHandle> = Vec::new();
    let mut all_lufs: Vec<LufsHandle> = Vec::new();
    let mut registry = EffectRegistry::new();
    for out in &graph.outputs {
        let output_sr = output_runtime
            .get(&out.id)
            .map(|o| o.sample_rate())
            .ok_or_else(|| AppError::Validation("missing output runtime".into()))?;
        let built = build_output_graph(
            Some(out.id.as_str()),
            output_sr,
            graph,
            &input_native_sr,
            &mut producers_by_input,
            &mut registry,
        )?;
        for (id, control) in built.controls {
            effect_controls.entry(id).or_insert(control);
        }
        all_meters.extend(built.meters);
        all_lufs.extend(built.lufs);
        output_graphs.insert(out.id.clone(), built.graph);
    }

    let mut monitor_graph: Option<OutputGraph> = None;
    if monitor_mode {
        // Highest input SR — don't downsample any source feeding the meter.
        let monitor_sr = input_native_sr.values().copied().max().unwrap_or(48_000);
        let built = build_output_graph(
            None,
            monitor_sr,
            graph,
            &input_native_sr,
            &mut producers_by_input,
            &mut registry,
        )?;
        for (id, control) in built.controls {
            effect_controls.entry(id).or_insert(control);
        }
        all_meters.extend(built.meters);
        all_lufs.extend(built.lufs);
        monitor_graph = Some(built.graph);
    }

    let mut input_streams = Vec::with_capacity(input_runtime.len());
    for (input_id, resolved) in input_runtime {
        let producers = producers_by_input.remove(&input_id).unwrap_or_default();
        if producers.is_empty() {
            continue;
        }
        let meter = MeterHandle::new(input_id.clone());
        all_meters.push(meter.clone());
        let stream = start_input_stream(resolved, producers, meter, &app)?;
        input_streams.push(stream);
    }

    let mut speaker_streams = Vec::new();
    let mut workers = Vec::new();
    for out in &graph.outputs {
        let resolved = match output_runtime.remove(&out.id) {
            Some(r) => r,
            None => continue,
        };
        let og = match output_graphs.remove(&out.id) {
            Some(g) => g,
            None => continue,
        };
        match resolved {
            ResolvedOutput::Speaker(spec) => {
                speaker_streams.push(start_speaker_stream(spec, og, &app)?);
            }
            ResolvedOutput::File { path, sample_rate } => {
                workers.push(start_recorder_worker(
                    out.id.clone(),
                    path,
                    sample_rate,
                    og,
                    app.clone(),
                )?);
            }
        }
    }
    if let Some(og) = monitor_graph {
        workers.push(start_monitor_worker(og)?);
    }

    info!(
        inputs = input_streams.len(),
        speakers = speaker_streams.len(),
        recorders = workers.len(),
        outputs = graph.outputs.len(),
        effects = graph.effects.len(),
        edges = graph.edges.len(),
        "pipeline started"
    );

    let meter_thread = if all_meters.is_empty() && all_lufs.is_empty() {
        None
    } else {
        Some(spawn_meter_thread(app.clone(), all_meters, all_lufs))
    };

    Ok(ActivePipeline {
        _input_streams: input_streams,
        _speaker_streams: speaker_streams,
        _workers: workers,
        effect_controls,
        _meter_thread: meter_thread,
    })
}

struct BuiltOutputGraph {
    graph: OutputGraph,
    controls: Vec<(String, EffectControl)>,
    meters: Vec<MeterHandle>,
    lufs: Vec<LufsHandle>,
}

/// Build the per-output DAG: walk backward from `output_id`, topo-sort the
/// reachable sub-graph, instantiate sources (with their rings) and effects
/// (with their parameter atomics) in order.
///
/// `output_id = None` means monitor mode: every surviving input + effect is
/// reachable (validate already trimmed anything that doesn't drive an
/// analyzer), and the resulting graph has no output terminals.
fn build_output_graph(
    output_id: Option<&str>,
    output_sr: u32,
    valid: &ValidGraph,
    input_native_sr: &HashMap<String, u32>,
    producers_by_input: &mut HashMap<String, Vec<Producer<f32>>>,
    registry: &mut EffectRegistry,
) -> AppResult<BuiltOutputGraph> {
    let reachable: HashSet<String> = match output_id {
        Some(id) => reachable_backward(id, valid),
        None => valid
            .inputs
            .iter()
            .map(|i| i.id.clone())
            .chain(valid.effects.iter().map(|e| e.id.clone()))
            .collect(),
    };

    // Topo sort restricted to the reachable sub-graph. Inputs have indegree 0
    // within the sub-graph; outputs are excluded entirely (they're not DAG
    // nodes here, just sinks).
    let mut indegree: HashMap<String, usize> = HashMap::new();
    for id in &reachable {
        indegree.entry(id.clone()).or_insert(0);
    }
    for edge in &valid.edges {
        if reachable.contains(&edge.from) && reachable.contains(&edge.to) {
            *indegree.entry(edge.to.clone()).or_insert(0) += 1;
        }
    }
    let mut queue: Vec<String> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(id, _)| id.clone())
        .collect();
    queue.sort(); // deterministic order
    let mut topo: Vec<String> = Vec::with_capacity(reachable.len());
    while let Some(id) = queue.pop() {
        topo.push(id.clone());
        for edge in &valid.edges {
            if edge.from == id && reachable.contains(&edge.to) {
                let d = indegree.get_mut(&edge.to).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push(edge.to.clone());
                }
            }
        }
    }
    if topo.len() != reachable.len() {
        return Err(AppError::Validation(format!(
            "internal: topo sort failed for output {}",
            output_id.unwrap_or("<monitor>")
        )));
    }

    // Build nodes in topo order. `id_to_index` lets effects resolve their
    // upstream node positions in the final Vec.
    let mut nodes: Vec<DagNode> = Vec::with_capacity(topo.len());
    let mut id_to_index: HashMap<String, usize> = HashMap::new();
    let mut controls: Vec<(String, EffectControl)> = Vec::new();
    let mut meters: Vec<MeterHandle> = Vec::new();
    let mut lufs: Vec<LufsHandle> = Vec::new();
    // Frames of accumulated delay at each node's output.
    let mut node_latencies: Vec<usize> = Vec::with_capacity(topo.len());

    for id in &topo {
        if let Some(_input) = valid.inputs.iter().find(|i| &i.id == id) {
            let input_sr = *input_native_sr
                .get(id)
                .ok_or_else(|| AppError::Validation(format!("input {id} has no SR")))?;
            let (producer, consumer) = RingBuffer::<f32>::new(RING_CAPACITY);
            producers_by_input.entry(id.clone()).or_default().push(producer);

            let resampler = if input_sr == output_sr {
                None
            } else {
                Some(StereoResampler::new(input_sr, output_sr, RESAMPLE_CHUNK)?)
            };
            let out_max = resampler.as_ref().map(|r| r.out_max()).unwrap_or(RESAMPLE_CHUNK);
            // ×4 headroom: one chunk draining + one in-flight + alignment slack.
            let staging_cap = out_max * 4 + DSP_BLOCK_FRAMES * 2;
            let input_frames_per_block = (DSP_BLOCK_FRAMES as u64 * input_sr as u64
                + output_sr as u64
                - 1)
                / output_sr as u64;
            let input_samples_per_block = (input_frames_per_block as usize) * 2;

            let source = SourceState {
                label: format!("{id}@{input_sr}->{output_sr}"),
                consumer,
                resampler,
                input_staging: Vec::with_capacity(RESAMPLE_CHUNK * 2 + 8),
                out_pending: StagingRing::with_capacity(staging_cap),
                chunk_tmp: Vec::with_capacity(out_max * 2),
                out_buf: vec![0.0; DSP_BLOCK_FRAMES * 2],
                input_samples_per_block,
                last_pop_at: Instant::now(),
                first_data_logged: false,
            };
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Source(source));
            node_latencies.push(0);
        } else if let Some(effect) = valid.effects.iter().find(|e| &e.id == id) {
            let build = instantiate_effect(&effect.spec, id, output_sr, registry);
            if let Some(c) = build.control {
                controls.push((id.clone(), c));
            }
            if let Some(m) = build.meter {
                meters.push(m);
            }
            if let Some(l) = build.lufs {
                lufs.push(l);
            }
            let upstream: Vec<usize> = valid
                .edges
                .iter()
                .filter(|e| &e.to == id && reachable.contains(&e.from))
                .map(|e| id_to_index[&e.from])
                .collect();
            let max_upstream = upstream
                .iter()
                .map(|&i| node_latencies[i])
                .max()
                .unwrap_or(0);
            let incoming: Vec<IncomingEdge> = upstream
                .iter()
                .map(|&src_idx| {
                    let pad = max_upstream - node_latencies[src_idx];
                    IncomingEdge {
                        src_idx,
                        delay: if pad > 0 { Some(DelayLine::new(pad)) } else { None },
                    }
                })
                .collect();
            let own = build.effect.latency_frames();
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Effect(EffectState {
                effect: build.effect,
                incoming,
                out_buf: vec![0.0; DSP_BLOCK_FRAMES * 2],
            }));
            node_latencies.push(max_upstream + own);
        }
    }

    let terminals: Vec<TerminalEdge> = match output_id {
        Some(id) => {
            let upstream: Vec<usize> = valid
                .edges
                .iter()
                .filter(|e| e.to == id)
                .filter_map(|e| id_to_index.get(&e.from).copied())
                .collect();
            let max_upstream = upstream
                .iter()
                .map(|&i| node_latencies[i])
                .max()
                .unwrap_or(0);
            upstream
                .into_iter()
                .map(|src_idx| {
                    let pad = max_upstream - node_latencies[src_idx];
                    TerminalEdge {
                        src_idx,
                        delay: if pad > 0 { Some(DelayLine::new(pad)) } else { None },
                    }
                })
                .collect()
        }
        None => Vec::new(),
    };

    Ok(BuiltOutputGraph {
        graph: OutputGraph {
            sample_rate: output_sr,
            nodes,
            terminals,
        },
        controls,
        meters,
        lufs,
    })
}

/// Node ids reachable backward from `output_id`, excluding the output node itself.
fn reachable_backward(output_id: &str, valid: &ValidGraph) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack: Vec<String> = valid
        .edges
        .iter()
        .filter(|e| e.to == output_id)
        .map(|e| e.from.clone())
        .collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for edge in &valid.edges {
            if edge.to == id {
                stack.push(edge.from.clone());
            }
        }
    }
    seen
}

fn inputs_feeding_output<'a>(output_id: &str, valid: &'a ValidGraph) -> Vec<&'a str> {
    let reachable = reachable_backward(output_id, valid);
    valid
        .inputs
        .iter()
        .filter(|i| reachable.contains(&i.id))
        .map(|i| i.id.as_str())
        .collect()
}

// ---------- input resolution ----------

enum ResolvedInput {
    Cpal {
        device: cpal::Device,
        config: cpal::StreamConfig,
        sample_format: cpal::SampleFormat,
        src_channels: usize,
        sample_rate: u32,
    },
    SystemAudio {
        sample_rate: u32,
        exclude_current_app: bool,
    },
    AppAudio {
        sample_rate: u32,
        bundle_id: String,
    },
}

impl ResolvedInput {
    fn sample_rate(&self) -> u32 {
        match self {
            ResolvedInput::Cpal { sample_rate, .. } => *sample_rate,
            ResolvedInput::SystemAudio { sample_rate, .. } => *sample_rate,
            ResolvedInput::AppAudio { sample_rate, .. } => *sample_rate,
        }
    }
}

fn resolve_input(inp: &ValidInput) -> AppResult<ResolvedInput> {
    match &inp.spec {
        InputSpec::Microphone { device_id } => {
            let device = device::find(DeviceKind::Input, device_id)?;
            let native = native_config(DeviceKind::Input, &device, device_id)?;
            Ok(ResolvedInput::Cpal {
                device,
                config: native.config,
                sample_format: native.sample_format,
                src_channels: native.channels as usize,
                sample_rate: native.sample_rate,
            })
        }
        InputSpec::SystemAudio {
            exclude_current_app,
        } => Ok(ResolvedInput::SystemAudio {
            sample_rate: SCK_SR,
            exclude_current_app: *exclude_current_app,
        }),
        InputSpec::AppAudio { bundle_id } => Ok(ResolvedInput::AppAudio {
            sample_rate: SCK_SR,
            bundle_id: bundle_id.clone(),
        }),
    }
}

fn start_input_stream(
    resolved: ResolvedInput,
    producers: Vec<Producer<f32>>,
    meter: MeterHandle,
    app: &AppHandle,
) -> AppResult<InputHandle> {
    let app_err = app.clone();
    let err_cb = move |e: cpal::StreamError| {
        let _ = app_err.emit(
            STATE_EVENT,
            json!({ "kind": "error", "message": format!("input: {e}") }),
        );
    };

    match resolved {
        ResolvedInput::Cpal {
            device,
            config,
            sample_format,
            src_channels,
            ..
        } => {
            let stream = streams::build_input_stream(
                &device,
                &config,
                sample_format,
                src_channels,
                producers,
                Some(meter),
                err_cb,
            )?;
            Ok(InputHandle::Cpal(stream))
        }
        #[cfg(target_os = "macos")]
        ResolvedInput::SystemAudio {
            sample_rate,
            exclude_current_app,
        } => {
            info!(
                sample_rate,
                exclude_current_app, "starting system-audio capture (ScreenCaptureKit)"
            );
            let capture = crate::audio::sck_capture::SckCapture::start_system(
                exclude_current_app,
                sample_rate,
                SCK_CHANNELS as u32,
                producers,
                Some(meter),
            )?;
            Ok(InputHandle::Sck(capture))
        }
        #[cfg(target_os = "macos")]
        ResolvedInput::AppAudio {
            sample_rate,
            bundle_id,
        } => {
            info!(sample_rate, %bundle_id, "starting app-audio capture (ScreenCaptureKit)");
            let capture = crate::audio::sck_capture::SckCapture::start_app(
                &bundle_id,
                sample_rate,
                SCK_CHANNELS as u32,
                producers,
                Some(meter),
            )?;
            Ok(InputHandle::Sck(capture))
        }
        #[cfg(not(target_os = "macos"))]
        ResolvedInput::SystemAudio { .. } | ResolvedInput::AppAudio { .. } => {
            drop(producers);
            let _ = meter;
            Err(AppError::Stream(
                "System/App Audio capture is only supported on macOS".into(),
            ))
        }
    }
}

/// ScreenCaptureKit always delivers interleaved stereo by configuration.
const SCK_CHANNELS: usize = 2;
/// ScreenCaptureKit sample rate request. 48 kHz is macOS's universal audio
/// rate and matches AVAudioSession / CoreAudio's preferred output, so no
/// resampling happens on the SCK delivery side.
const SCK_SR: u32 = 48_000;

// ---------- output resolution ----------

struct SpeakerResolved {
    device: cpal::Device,
    config: cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    out_channels: usize,
    sample_rate: u32,
}

enum ResolvedOutput {
    Speaker(SpeakerResolved),
    File {
        path: PathBuf,
        sample_rate: u32,
    },
}

impl ResolvedOutput {
    fn sample_rate(&self) -> u32 {
        match self {
            ResolvedOutput::Speaker(s) => s.sample_rate,
            ResolvedOutput::File { sample_rate, .. } => *sample_rate,
        }
    }
}

fn resolve_output(out: &ValidOutput, file_sr_hint: Option<u32>) -> AppResult<ResolvedOutput> {
    match &out.spec {
        OutputSpec::Speaker { device_id } => {
            let device = device::find(DeviceKind::Output, device_id)?;
            let native = native_config(DeviceKind::Output, &device, device_id)?;
            Ok(ResolvedOutput::Speaker(SpeakerResolved {
                device,
                config: native.config,
                sample_format: native.sample_format,
                out_channels: native.channels as usize,
                sample_rate: native.sample_rate,
            }))
        }
        OutputSpec::FileRecording { file_path } => Ok(ResolvedOutput::File {
            path: PathBuf::from(file_path),
            // Prefer the highest source rate so the WAV never carries a
            // downsampled signal. Fall back to 48 kHz only when there's no
            // input connected (engine will then error out anyway).
            sample_rate: file_sr_hint.unwrap_or(RECORDER_DEFAULT_SR),
        }),
    }
}

// ---------- native config resolution ----------
//
// We never ask cpal "what is this device's default/supported config?":
//   - `default_*_config` reads the *currently active* CoreAudio stream format,
//     which is absent for non-default routes (built-in speakers while AirPods
//     are connected) → "Invalid property value".
//   - `supported_*_configs` reads `kAudioStreamPropertyAvailableVirtualFormats`,
//     which is also empty for those same non-default routes.
//
// AUHAL (cpal's underlying output unit on macOS) does NOT need to be told the
// device's "current" format up front — it accepts whatever StreamConfig we
// hand it and asks CoreAudio to convert. So we read the device's nominal
// sample rate and channel count *directly* from CoreAudio HAL (which works
// regardless of routing state) and feed those into `build_*_stream`.
//
// Sample format is always `f32` — the universal macOS audio type and the
// internal pipeline format.

struct NativeConfig {
    config: cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    sample_rate: u32,
    channels: u16,
}

#[cfg(target_os = "macos")]
fn native_config(
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
fn native_config(
    kind: DeviceKind,
    device: &cpal::Device,
    name: &str,
) -> AppResult<NativeConfig> {
    // On Linux/Windows cpal's `supported_*_configs` is reliable for any device
    // the OS exposes — no inactive-route quirk like macOS. Pick the range with
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

// ---------- output runtime: speakers ----------

/// Output-side ring for the speaker pipeline. DSP worker pushes mixed stereo;
/// the cpal callback drains. 32k samples = 16k stereo frames ≈ 340 ms @ 48 k —
/// massive headroom for cpal/scheduler jitter, costs ~128 KB.
const SPEAKER_RING_CAPACITY: usize = 32_768;

/// macOS Bluetooth often fails first AUHAL bind with DeviceNotAvailable even
/// when active; 3×300ms covers settling.
const SPEAKER_MAX_ATTEMPTS: u32 = 3;
const SPEAKER_RETRY_DELAY: Duration = Duration::from_millis(300);

// Substring match on cpal's stable Display — AppError flattens the variant.
fn is_device_not_available(e: &AppError) -> bool {
    matches!(e, AppError::Stream(s) if s.contains("no longer available"))
}

/// Bundles the cpal stream with the DSP worker that feeds its ring. `Drop`
/// stops cpal first (so the callback can't read stale memory mid-shutdown),
/// then signals the worker and joins. Field order matters.
struct SpeakerHandle {
    _stream: cpal::Stream,
    _worker: SpeakerWorker,
}

struct SpeakerWorker {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for SpeakerWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn start_speaker_stream(
    spec: SpeakerResolved,
    graph: OutputGraph,
    app: &AppHandle,
) -> AppResult<SpeakerHandle> {
    use cpal::traits::DeviceTrait;
    let device_name = spec
        .device
        .name()
        .unwrap_or_else(|_| "<unknown>".into());
    info!(
        device = %device_name,
        sample_rate = spec.sample_rate,
        channels = spec.out_channels,
        format = ?spec.sample_format,
        "opening speaker stream",
    );

    // AirPods A2DP↔HFP switch can race resolve_output; verify state fresh.
    #[cfg(target_os = "macos")]
    {
        let fresh = crate::audio::macos_hal::find_output_device(&device_name);
        match fresh {
            None => warn!(device = %device_name, "HAL no longer sees the device"),
            Some(hal) if hal.sample_rate != spec.sample_rate => warn!(
                device = %device_name,
                resolved_sample_rate = spec.sample_rate,
                current_sample_rate = hal.sample_rate,
                "device sample rate changed between resolve and open"
            ),
            Some(hal) if hal.channels as usize != spec.out_channels => warn!(
                device = %device_name,
                resolved_channels = spec.out_channels,
                current_channels = hal.channels,
                "device channel count changed between resolve and open"
            ),
            Some(_) => {}
        }
    }

    // cpal consumes the closures on each attempt, so ring + closures are
    // recreated per iteration.
    let mut producer_holder: Option<Producer<f32>> = None;
    let mut stream_holder: Option<cpal::Stream> = None;
    for attempt in 1..=SPEAKER_MAX_ATTEMPTS {
        let (producer, mut consumer) = RingBuffer::<f32>::new(SPEAKER_RING_CAPACITY);
        let fill = move |stereo_out: &mut [f32], _frames: usize| {
            streams::bulk_pop(&mut consumer, stereo_out);
        };
        let app_err = app.clone();
        let err_cb = move |e: cpal::StreamError| {
            let _ = app_err.emit(
                STATE_EVENT,
                json!({ "kind": "error", "message": format!("output: {e}") }),
            );
        };
        match streams::build_output_stream(
            &spec.device,
            &spec.config,
            spec.sample_format,
            spec.out_channels,
            fill,
            err_cb,
        ) {
            Ok(s) => {
                producer_holder = Some(producer);
                stream_holder = Some(s);
                break;
            }
            Err(e) if attempt < SPEAKER_MAX_ATTEMPTS && is_device_not_available(&e) => {
                warn!(
                    attempt,
                    error = %e,
                    "DeviceNotAvailable from cpal; retrying after delay"
                );
                thread::sleep(SPEAKER_RETRY_DELAY);
            }
            Err(e) => return Err(e),
        }
    }
    let mut producer = producer_holder.expect("loop sets producer on success or returns Err");
    let stream = stream_holder.expect("loop sets stream on success or returns Err");

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let worker = DspWorker { graph };
    let clock: Box<dyn ClockSource> =
        Box::new(SystemClockTicker::new(spec.sample_rate, DSP_BLOCK_FRAMES));
    let pacing = WorkerPacing::Clock(clock);
    let join = thread::Builder::new()
        .name(format!("speaker:{}", spec.sample_rate))
        .spawn(move || {
            worker.run(stop_thread, pacing, |block| {
                streams::bulk_push(&mut producer, block);
                Ok(())
            });
        })
        .map_err(|e| AppError::Stream(format!("spawn speaker worker: {e}")))?;

    Ok(SpeakerHandle {
        _stream: stream,
        _worker: SpeakerWorker {
            stop,
            join: Some(join),
        },
    })
}

// ---------- meter tick thread ----------

fn spawn_meter_thread(
    app: AppHandle,
    meters: Vec<MeterHandle>,
    lufs: Vec<LufsHandle>,
) -> MeterTickThread {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let join = thread::Builder::new()
        .name("meter-tick".into())
        .spawn(move || {
            while !stop_thread.load(Ordering::SeqCst) {
                thread::sleep(METER_TICK);
                for m in &meters {
                    let snap = m.snapshot_and_decay();
                    let _ = app.emit(
                        METER_EVENT,
                        json!({
                            "nodeId": m.node_id,
                            "peakL": snap.peak_l,
                            "peakR": snap.peak_r,
                            "rmsL": snap.rms_l,
                            "rmsR": snap.rms_r,
                        }),
                    );
                }
                for l in &lufs {
                    let snap = l.snapshot();
                    let _ = app.emit(
                        LUFS_EVENT,
                        json!({
                            "nodeId": l.node_id,
                            "momentary": snap.momentary,
                            "shortterm": snap.shortterm,
                            "integrated": snap.integrated,
                            "tpL": snap.tp_l,
                            "tpR": snap.tp_r,
                        }),
                    );
                }
            }
        })
        .expect("spawn meter tick thread");
    MeterTickThread {
        stop,
        join: Some(join),
    }
}

// Speaker and File outputs share the same DspWorker; the speaker's cpal
// callback is a plain ring drain, off the RT thread.

const DSP_BLOCK_FRAMES: usize = 1024;
/// Let input rings collect a few cpal buffers before starting the clock —
/// otherwise the first block is all zeros.
const DSP_PREROLL: Duration = Duration::from_millis(50);

struct DspWorker {
    graph: OutputGraph,
}

/// How a worker decides when to produce the next block.
///
/// `Clock` ticks on a steady wall-clock cadence — right for Speaker outputs
/// where the device clock pulls audio in real time and a missed block becomes
/// audible silence.
///
/// `OnAvailability` waits until every source has enough buffered input for a
/// full output block (with a short timeout so a stalled source eventually
/// proceeds with zero-fill rather than hanging the recording). Right for File
/// outputs where bursty sources like ScreenCaptureKit drift against any
/// wall-clock cadence — waiting for data eliminates the mid-recording dropouts
/// that come from draining a half-empty ring.
enum WorkerPacing {
    Clock(Box<dyn ClockSource>),
    OnAvailability,
}

/// Cap on how long an availability-paced worker waits for slow sources before
/// proceeding with whatever it has (zero-fill for the missing samples).
const AVAILABILITY_MAX_WAIT: Duration = Duration::from_millis(200);
const AVAILABILITY_POLL: Duration = Duration::from_millis(2);

impl DspWorker {
    fn run<F>(mut self, stop: Arc<AtomicBool>, mut pacing: WorkerPacing, mut sink: F)
    where
        F: FnMut(&[f32]) -> AppResult<()>,
    {
        thread::sleep(DSP_PREROLL);
        let mut block = vec![0.0_f32; DSP_BLOCK_FRAMES * 2];

        loop {
            let proceed = match &mut pacing {
                WorkerPacing::Clock(clock) => clock.wait_for_tick(&stop),
                WorkerPacing::OnAvailability => self.wait_until_ready(&stop),
            };
            if !proceed {
                break;
            }

            self.graph.process_block(&mut block);
            if let Err(e) = sink(&block) {
                warn!(error = %e, "DSP worker sink failed; stopping");
                break;
            }
        }
    }

    /// Poll the graph's source readiness with a brief sleep between checks.
    /// Returns `false` only on stop — a timeout falls through to `true` so the
    /// worker still produces a (possibly partial) block, keeping the recording
    /// timeline moving when a source has truly gone silent.
    fn wait_until_ready(&self, stop: &AtomicBool) -> bool {
        let started = std::time::Instant::now();
        loop {
            if stop.load(Ordering::SeqCst) {
                return false;
            }
            if self.graph.all_sources_ready() {
                return true;
            }
            if started.elapsed() >= AVAILABILITY_MAX_WAIT {
                return true;
            }
            thread::sleep(AVAILABILITY_POLL);
        }
    }
}

// Drives analyzers when there's no real output; sink discards the mix.
fn start_monitor_worker(graph: OutputGraph) -> AppResult<RecorderWorker> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let worker = DspWorker { graph };
    let pacing = WorkerPacing::OnAvailability;
    let join = thread::Builder::new()
        .name("monitor".into())
        .spawn(move || {
            worker.run(stop_thread, pacing, |_block| Ok(()));
        })
        .map_err(|e| AppError::Stream(format!("spawn monitor worker: {e}")))?;
    Ok(RecorderWorker {
        stop,
        join: Some(join),
    })
}

// ---------- output runtime: file recording ----------

fn start_recorder_worker(
    node_id: String,
    path: PathBuf,
    sample_rate: u32,
    graph: OutputGraph,
    app: AppHandle,
) -> AppResult<RecorderWorker> {
    let recorder = WavRecorder::create(&path, sample_rate)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let worker = DspWorker { graph };
    let pacing = WorkerPacing::OnAvailability;

    let join = thread::Builder::new()
        .name(format!("recorder:{}", path.display()))
        .spawn(move || {
            // A crash loses at most one flush interval of audio.
            const FLUSH_INTERVAL: Duration = Duration::from_secs(2);
            const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
            let mut last_flush = std::time::Instant::now();
            let mut last_progress = std::time::Instant::now();
            let mut frames_written: u64 = 0;
            let mut recorder = recorder;

            worker.run(stop_thread, pacing, |block| {
                recorder.write_stereo(block)?;
                frames_written += (block.len() / 2) as u64;

                if last_flush.elapsed() >= FLUSH_INTERVAL {
                    if let Err(e) = recorder.flush() {
                        warn!(error = %e, "wav flush failed");
                    }
                    last_flush = std::time::Instant::now();
                }
                if last_progress.elapsed() >= PROGRESS_INTERVAL {
                    let _ = app.emit(
                        "audio://recorder_progress",
                        json!({
                            "nodeId": node_id,
                            "frames": frames_written,
                            "sampleRate": sample_rate,
                        }),
                    );
                    last_progress = std::time::Instant::now();
                }
                Ok(())
            });

            let _ = app.emit(
                "audio://recorder_progress",
                json!({
                    "nodeId": node_id,
                    "frames": frames_written,
                    "sampleRate": sample_rate,
                    "stopped": true,
                }),
            );

            if let Err(e) = recorder.finalize() {
                warn!(error = %e, "wav finalize failed");
            }
        })
        .map_err(|e| AppError::Stream(format!("spawn recorder thread: {e}")))?;

    Ok(RecorderWorker {
        stop,
        join: Some(join),
    })
}

