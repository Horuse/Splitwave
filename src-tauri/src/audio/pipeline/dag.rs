use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};
use tracing::{info, warn};

use crate::audio::effects::{
    instantiate_effect, EffectControl, EffectRegistry, GrHandle, LufsHandle, MeterHandle,
    WaveformHandle, RuntimeEffect,
};
use crate::audio::graph::{EdgeKind, ValidGraph};
use crate::audio::resample::StereoResampler;
use crate::error::{AppError, AppResult};

/// Ring buffer length (in stereo f32 samples) per bridge. Sized for ~500 ms
/// of stereo audio at 96 kHz so the worker can ride out longer source pauses
/// (SCK silent gaps, scheduler hiccups) without overflowing the FAST source's
/// ring while waiting on a SLOW one.
pub(super) const RING_CAPACITY: usize = 96_000;

/// Block size used by the resampler. 256 frames @ 48 kHz ~ 5.3 ms.
pub(super) const RESAMPLE_CHUNK: usize = 256;

pub(super) const DSP_BLOCK_FRAMES: usize = 1024;

/// How long a source can go without delivering before the availability-paced
/// worker stops waiting on it. SCK in normal operation delivers every ~20 ms,
/// so 150 ms is ~7x headroom -- enough to avoid false positives on bursty
/// delivery, short enough that a real stall doesn't drown the FAST source's
/// ring buffer.
const STALL_THRESHOLD: Duration = Duration::from_millis(150);

/// Fixed-capacity FIFO; allocates once. Overrun clamps and counts drops --
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
    /// Per-block scratch -- receives one resampler chunk before going into staging.
    chunk_tmp: Vec<f32>,
    /// Current block (length = DSP_BLOCK_FRAMES * 2).
    out_buf: Vec<f32>,
    /// Input-side (raw input-SR, stereo) sample count needed to produce one
    /// full output block. Used by the availability-pacing worker to know when
    /// this source has enough buffered audio to safely contribute a block.
    input_samples_per_block: usize,
    /// Wall-time of the most recent successful pop. Used to detect "silent"
    /// sources -- if no samples have flowed for `STALL_THRESHOLD`, the worker
    /// stops waiting on this source so the OTHER sources don't end up with
    /// their rings overflowing while we hang on a dead one.
    last_pop_at: Instant,
    /// One-shot startup log so we can confirm samples actually start flowing.
    first_data_logged: bool,
}

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

    fn fill_block(&mut self) {
        let need = self.out_buf.len();
        let mut written = self.out_pending.pop_into(&mut self.out_buf[..]);
        while written < need {
            self.try_refill_one_chunk();
            if self.out_pending.len() == 0 {
                // Ring empty too -- zero-fill the rest (real underrun).
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
            // one atomic op per sample -- RT-friendly).
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
                return;
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
    bypass: Arc<AtomicBool>,
    incoming: Vec<IncomingEdge>,
    sidechain: Vec<IncomingEdge>,
    out_buf: Vec<f32>,
    sidechain_buf: Option<Vec<f32>>,
}

/// `delay` is `Some` when this path is shorter than the longest reaching the
/// same mixing point -- pads it for sample-alignment before summing.
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
pub(super) struct OutputGraph {
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
    pub(super) fn all_sources_ready(&self) -> bool {
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
    pub(super) fn process_block(&mut self, output: &mut [f32]) {
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
                if let Some(sc_buf) = eff.sidechain_buf.as_mut() {
                    for s in sc_buf.iter_mut() {
                        *s = 0.0;
                    }
                    for edge in &mut eff.sidechain {
                        let src = head[edge.src_idx].out_buf();
                        match &mut edge.delay {
                            Some(d) => d.process_and_add(src, sc_buf),
                            None => {
                                for (dst, sv) in sc_buf.iter_mut().zip(src.iter()) {
                                    *dst += *sv;
                                }
                            }
                        }
                    }
                }
                if !eff.bypass.load(Ordering::Relaxed) {
                    let sc_slice = eff.sidechain_buf.as_deref();
                    eff.effect
                        .process_with_sidechain(&mut eff.out_buf, sc_slice, DSP_BLOCK_FRAMES);
                }
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

pub(super) struct BuiltOutputGraph {
    pub graph: OutputGraph,
    pub controls: Vec<(String, EffectControl)>,
    pub bypasses: Vec<(String, Arc<AtomicBool>)>,
    pub meters: Vec<MeterHandle>,
    pub lufs: Vec<LufsHandle>,
    pub gr_handles: Vec<GrHandle>,
    pub scopes: Vec<WaveformHandle>,
}

/// Build the per-output DAG: walk backward from `output_id`, topo-sort the
/// reachable sub-graph, instantiate sources (with their rings) and effects
/// (with their parameter atomics) in order.
///
/// `output_id = None` means monitor mode: every surviving input + effect is
/// reachable (validate already trimmed anything that doesn't drive an
/// analyzer), and the resulting graph has no output terminals.
/// `producer_pairs` carries Producer ends of the ring per Source node,
/// paired with their input node id. Caller tags each pair with the owning
/// output id and routes them into the matching input's broadcast.
pub(super) fn build_output_graph(
    output_id: Option<&str>,
    output_sr: u32,
    valid: &ValidGraph,
    input_native_sr: &HashMap<String, u32>,
    producer_pairs: &mut Vec<(String, Producer<f32>)>,
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
    queue.sort();
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
    let mut bypasses: Vec<(String, Arc<AtomicBool>)> = Vec::new();
    let mut meters: Vec<MeterHandle> = Vec::new();
    let mut lufs: Vec<LufsHandle> = Vec::new();
    let mut gr_handles: Vec<GrHandle> = Vec::new();
    let mut scopes: Vec<WaveformHandle> = Vec::new();
    let mut node_latencies: Vec<usize> = Vec::with_capacity(topo.len());

    for id in &topo {
        if let Some(_input) = valid.inputs.iter().find(|i| &i.id == id) {
            let input_sr = *input_native_sr
                .get(id)
                .ok_or_else(|| AppError::Validation(format!("input {id} has no SR")))?;
            let (producer, consumer) = RingBuffer::<f32>::new(RING_CAPACITY);
            producer_pairs.push((id.clone(), producer));

            let resampler = if input_sr == output_sr {
                None
            } else {
                Some(StereoResampler::new(input_sr, output_sr, RESAMPLE_CHUNK)?)
            };
            let out_max = resampler.as_ref().map(|r| r.out_max()).unwrap_or(RESAMPLE_CHUNK);
            // x4 headroom: one chunk draining + one in-flight + alignment slack.
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
            if build.bypass_is_new {
                bypasses.push((id.clone(), build.bypass.clone()));
            }
            if let Some(m) = build.meter {
                meters.push(m);
            }
            if let Some(l) = build.lufs {
                lufs.push(l);
            }
            if let Some(g) = build.gr {
                gr_handles.push(g);
            }
            if let Some(s) = build.scope {
                scopes.push(s);
            }
            let bypass = build.bypass;
            let mut main_upstream: Vec<usize> = Vec::new();
            let mut side_upstream: Vec<usize> = Vec::new();
            for e in &valid.edges {
                if &e.to == id && reachable.contains(&e.from) {
                    let idx = id_to_index[&e.from];
                    match e.kind {
                        EdgeKind::Main => main_upstream.push(idx),
                        EdgeKind::Sidechain => side_upstream.push(idx),
                    }
                }
            }
            let max_upstream = main_upstream
                .iter()
                .chain(side_upstream.iter())
                .map(|&i| node_latencies[i])
                .max()
                .unwrap_or(0);
            let make_edge = |src_idx: usize| {
                let pad = max_upstream - node_latencies[src_idx];
                IncomingEdge {
                    src_idx,
                    delay: if pad > 0 { Some(DelayLine::new(pad)) } else { None },
                }
            };
            let incoming: Vec<IncomingEdge> = main_upstream.iter().copied().map(make_edge).collect();
            let sidechain: Vec<IncomingEdge> =
                side_upstream.iter().copied().map(make_edge).collect();
            let sidechain_buf = if sidechain.is_empty() {
                None
            } else {
                Some(vec![0.0; DSP_BLOCK_FRAMES * 2])
            };
            let own = build.effect.latency_frames();
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Effect(EffectState {
                effect: build.effect,
                bypass,
                incoming,
                sidechain,
                out_buf: vec![0.0; DSP_BLOCK_FRAMES * 2],
                sidechain_buf,
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
        bypasses,
        meters,
        lufs,
        gr_handles,
        scopes,
    })
}

/// Node ids reachable backward from `output_id`, excluding the output node itself.
pub(super) fn reachable_backward(output_id: &str, valid: &ValidGraph) -> HashSet<String> {
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

pub(super) fn inputs_feeding_output<'a>(output_id: &str, valid: &'a ValidGraph) -> Vec<&'a str> {
    let reachable = reachable_backward(output_id, valid);
    valid
        .inputs
        .iter()
        .filter(|i| reachable.contains(&i.id))
        .map(|i| i.id.as_str())
        .collect()
}
