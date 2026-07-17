use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};
use tracing::{info, warn};

use crate::audio::effects::{
    instantiate_effect, update_meter, EffectControl, EffectRegistry, GrHandle, LufsHandle,
    MeterHandle, WaveformHandle, RuntimeEffect,
};
use crate::audio::graph::{EdgeKind, EffectSpec, InputSpec, NetCodec, OutputSpec, ValidGraph};
use crate::audio::netaudio::packet::Format;
use crate::audio::resample::MultiResampler;
use crate::audio::stream_recv::ChannelReceiver;
use crate::audio::streams::bulk_push;
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

const SOURCE_BACKLOG_HIGH_BLOCKS: usize = 4;
const SOURCE_BACKLOG_LOW_BLOCKS: usize = 2;

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

    fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.len = 0;
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
/// `Effect` sums its upstreams' buffers and runs DSP, `Producer` emits
/// network-received audio on named channel handles. Each exposes an
/// interleaved `out_buf` of `DSP_BLOCK_FRAMES * node_channels` that downstream
/// nodes consume.
enum DagNode {
    Source(SourceState),
    Effect(EffectState),
    Producer(ProducerState),
    Consumer(ConsumerState),
}

impl DagNode {
    fn out_buf(&self) -> &[f32] {
        match self {
            DagNode::Source(s) => &s.out_buf,
            DagNode::Effect(e) => &e.out_buf,
            DagNode::Producer(p) => &p.out_buf,
            // Terminal sink; validation forbids outgoing edges, so never read.
            DagNode::Consumer(_) => &[],
        }
    }

    /// Unknown or absent handles fall back to the node's main `out_buf`.
    fn out_buf_for_handle(&self, handle: Option<&str>) -> &[f32] {
        let (handle_bufs, out_buf) = match self {
            DagNode::Effect(e) => (&e.handle_bufs, &e.out_buf),
            DagNode::Producer(p) => (&p.handle_bufs, &p.out_buf),
            DagNode::Source(s) => (&s.handle_bufs, &s.out_buf),
            DagNode::Consumer(_) => return self.out_buf(),
        };
        match handle {
            Some(h) => handle_bufs
                .iter()
                .find(|(id, _)| id == h)
                .map(|(_, buf)| buf.as_slice())
                .unwrap_or(out_buf),
            None => out_buf,
        }
    }
}

struct SourceState {
    label: String,
    channels: usize,
    consumer: Consumer<f32>,
    resampler: Option<MultiResampler>,
    input_staging: Vec<f32>,
    out_pending: StagingRing,
    chunk_tmp: Vec<f32>,
    out_buf: Vec<f32>,
    input_samples_per_block: usize,
    realtime: bool,
    /// >STALL_THRESHOLD since last pop => zero-fill and stop waiting on this source.
    last_pop_at: Instant,
    first_data_logged: bool,
    volume: Arc<AtomicU32>,
    paused: Option<Arc<AtomicBool>>,
    // u64 generation (not AtomicBool) so every output's SourceState detects the
    // seek independently; swap(false) would clear the flag for the first reader.
    drain: Option<Arc<AtomicU64>>,
    last_drain_gen: u64,
    meter: Option<MeterHandle>,
    // Per-channel taps ("chK") drawn off this source, each a stereo buffer with
    // the single physical channel duplicated L=R.
    handle_bufs: Vec<(String, Vec<f32>)>,
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
        if let Some(p) = &self.paused {
            if p.load(Ordering::SeqCst) {
                return true;
            }
        }
        if self.is_stalled() {
            return true;
        }
        let have = self.consumer.slots() + self.input_staging.len();
        have >= self.input_samples_per_block
    }

    fn fill_block(&mut self) {
        if let Some(p) = &self.paused {
            if p.load(Ordering::SeqCst) {
                let avail = self.consumer.slots();
                if avail > 0 {
                    if let Ok(chunk) = self.consumer.read_chunk(avail) {
                        chunk.commit_all();
                    }
                }
                self.input_staging.clear();
                self.out_pending.clear();
                self.out_buf.fill(0.0);
                return;
            }
        }
        if let Some(d) = &self.drain {
            let gen = d.load(Ordering::SeqCst);
            if gen != self.last_drain_gen {
                self.last_drain_gen = gen;
                let avail = self.consumer.slots();
                if avail > 0 {
                    if let Ok(chunk) = self.consumer.read_chunk(avail) {
                        chunk.commit_all();
                    }
                }
                self.input_staging.clear();
                self.out_pending.clear();
                self.out_buf.fill(0.0);
                return;
            }
        }
        // Drop input backlog past HIGH down to LOW so latency stays bounded.
        if self.realtime {
            let have = self.consumer.slots();
            let high = self.input_samples_per_block * SOURCE_BACKLOG_HIGH_BLOCKS;
            if have > high {
                let low = self.input_samples_per_block * SOURCE_BACKLOG_LOW_BLOCKS;
                let excess = have - low;
                let drop = excess - excess % self.channels;
                if let Ok(chunk) = self.consumer.read_chunk(drop) {
                    chunk.commit_all();
                }
            }
        }
        let need = self.out_buf.len();
        let mut written = self.out_pending.pop_into(&mut self.out_buf[..]);
        while written < need {
            self.try_refill_one_chunk();
            if self.out_pending.len() == 0 {
                // Ring empty too -- zero-fill the rest (real underrun).
                for s in &mut self.out_buf[written..] {
                    *s = 0.0;
                }
                break;
            }
            let n = self.out_pending.pop_into(&mut self.out_buf[written..]);
            written += n;
        }
        const ONE_BITS: u32 = 0x3F80_0000;
        let vol_bits = self.volume.load(Ordering::Relaxed);
        if vol_bits != ONE_BITS {
            let vol = f32::from_bits(vol_bits);
            for s in self.out_buf.iter_mut() {
                *s *= vol;
            }
        }
        if let Some(m) = &self.meter {
            update_meter(m, &self.out_buf, self.channels);
        }
        if !self.handle_bufs.is_empty() {
            let w = self.channels;
            for (h, buf) in self.handle_bufs.iter_mut() {
                if let Some(a) = parse_stereo(h) {
                    let c0 = (a - 1).min(w - 1);
                    let c1 = a.min(w - 1);
                    for f in 0..DSP_BLOCK_FRAMES {
                        buf[f * 2] = self.out_buf[f * w + c0];
                        buf[f * 2 + 1] = self.out_buf[f * w + c1];
                    }
                } else {
                    let c = parse_ch(h).map(|k| (k - 1).min(w - 1)).unwrap_or(0);
                    for f in 0..DSP_BLOCK_FRAMES {
                        buf[f] = self.out_buf[f * w + c];
                    }
                }
            }
        }
    }

    fn try_refill_one_chunk(&mut self) {
        if let Some(rs) = &mut self.resampler {
            let needed = rs.chunk_in() * self.channels;
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
            let want = RESAMPLE_CHUNK * self.channels;
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
        // Whole-frame guarantee (don't split a frame across channels).
        let frames = self.chunk_tmp.len() / self.channels;
        self.chunk_tmp.truncate(frames * self.channels);
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
    // One instance per stereo pair; each carries its own DSP state but shares
    // the parameter atomics. `effects[0]` alone for width <= 2.
    effects: Vec<RuntimeEffect>,
    // Analyzers (level meter) read the whole N-wide buffer at once instead of
    // being split into stereo pairs, so they report every channel.
    full_width: bool,
    bypass: Arc<AtomicBool>,
    incoming: Vec<IncomingEdge>,
    sidechain: Vec<IncomingEdge>,
    out_buf: Vec<f32>,
    sidechain_buf: Option<Vec<f32>>,
    // Scratch for deinterleaving one pair out of a >2-wide buffer.
    pair_main: Vec<f32>,
    pair_side: Vec<f32>,
    handle_bufs: Vec<(String, Vec<f32>)>,
    // WebRTC bridge only: one input buffer per send channel ("ch1".."chN"),
    // summed by target handle and pushed to the matching send ring.
    channel_bufs: Vec<(String, Vec<f32>)>,
}

impl EffectState {
    /// Run the effect chain over `out_buf`. Width <= 2 processes in place; wider
    /// buffers are split into stereo pairs, each through its own instance so
    /// per-channel filter state never bleeds across pairs.
    fn run(&mut self, frames: usize) {
        let w = self.out_buf.len() / frames;
        if self.full_width || w == 2 {
            let sc = self.sidechain_buf.as_deref();
            self.effects[0].process_with_sidechain(&mut self.out_buf, sc, frames);
            return;
        }
        for p in 0..self.effects.len() {
            let (c0, c1) = (2 * p, 2 * p + 1);
            for f in 0..frames {
                let base = f * w;
                self.pair_main[f * 2] = self.out_buf[base + c0];
                self.pair_main[f * 2 + 1] = if c1 < w { self.out_buf[base + c1] } else { 0.0 };
            }
            let sc = if let Some(scb) = self.sidechain_buf.as_ref() {
                for f in 0..frames {
                    let base = f * w;
                    self.pair_side[f * 2] = scb[base + c0];
                    self.pair_side[f * 2 + 1] = if c1 < w { scb[base + c1] } else { 0.0 };
                }
                Some(self.pair_side.as_slice())
            } else {
                None
            };
            self.effects[p].process_with_sidechain(&mut self.pair_main, sc, frames);
            for f in 0..frames {
                let base = f * w;
                self.out_buf[base + c0] = self.pair_main[f * 2];
                if c1 < w {
                    self.out_buf[base + c1] = self.pair_main[f * 2 + 1];
                }
            }
        }
    }
}

/// A source with no graph inputs that emits network-received audio: `out_buf`
/// is the mix of every channel, `handle_bufs` are the per-channel outputs (keyed
/// by the source handle id, which matches the tap key).
struct ProducerState {
    receiver: ChannelReceiver,
    out_buf: Vec<f32>,
    handle_bufs: Vec<(String, Vec<f32>)>,
}

impl ProducerState {
    fn process(&mut self) {
        self.receiver.mix_block(&mut self.out_buf);
        for (handle, buf) in self.handle_bufs.iter_mut() {
            self.receiver.channel(handle, buf);
        }
    }

    fn is_ready_for_block(&self) -> bool {
        self.receiver.ready(DSP_BLOCK_FRAMES * 2)
    }
}

/// A terminal sink that consumes per-channel inputs (summed by target handle
/// into `channel_bufs`, keyed "ch1".."chN") and pushes each channel into its
/// send ring for a background transmitter (direct-IP NetSender).
struct ConsumerState {
    incoming: Vec<IncomingEdge>,
    channel_bufs: Vec<(String, Vec<f32>)>,
    send_producers: Vec<Producer<f32>>,
}

/// `delay` is `Some` when this path is shorter than the longest reaching the
/// same mixing point -- pads it for sample-alignment before summing.
struct IncomingEdge {
    src_idx: usize,
    source_handle: Option<String>,
    target_handle: Option<String>,
    /// `Some(off)` places this input at channel `off` (monitors stack their
    /// inputs side by side instead of summing them onto shared channels).
    stack_ch: Option<usize>,
    delay: Option<DelayLine>,
}

struct TerminalEdge {
    src_idx: usize,
    source_handle: Option<String>,
    /// `Some((off, width))` routes this edge to a physical output block: width 1
    /// (`chK`) downmixes to mono at `off`, width 2 (`stA`) places a stereo pair.
    route: Option<(usize, usize)>,
    delay: Option<DelayLine>,
}

/// Parse a `chK` handle into its 1-based channel number.
#[inline]
fn parse_ch(handle: &str) -> Option<usize> {
    handle.strip_prefix("ch").and_then(|s| s.parse::<usize>().ok())
}

/// Parse an `stA` stereo-group handle into its 1-based lower channel; the group
/// carries channels A and A+1.
#[inline]
fn parse_stereo(handle: &str) -> Option<usize> {
    handle.strip_prefix("st").and_then(|s| s.parse::<usize>().ok())
}

/// A per-channel tap handle (`chK` mono or `stA` stereo) and its channel width.
#[inline]
fn tap_handle_width(handle: &str) -> Option<usize> {
    if parse_stereo(handle).is_some() {
        Some(2)
    } else if parse_ch(handle).is_some() {
        Some(1)
    } else {
        None
    }
}

/// Route a target handle to a `(physical channel offset, width)` block: `chK`
/// lands on one channel, `stA` on the pair starting at A.
#[inline]
fn target_route(handle: &str) -> Option<(usize, usize)> {
    if let Some(a) = parse_stereo(handle) {
        Some((a - 1, 2))
    } else if let Some(k) = parse_ch(handle) {
        Some((k - 1, 1))
    } else {
        None
    }
}

/// Sum `src` into `dst` mapping channel-for-channel when the two have different
/// widths (min of the two; extra source channels dropped, extra dest channels
/// left untouched). Widths are inferred from length: `len / DSP_BLOCK_FRAMES`.
#[inline]
fn add_mapped(src: &[f32], dst: &mut [f32]) {
    let src_ch = src.len() / DSP_BLOCK_FRAMES;
    let dst_ch = dst.len() / DSP_BLOCK_FRAMES;
    if src_ch == 0 || dst_ch == 0 {
        return;
    }
    if src_ch == dst_ch {
        for (d, &s) in dst.iter_mut().zip(src.iter()) {
            *d += s;
        }
        return;
    }
    if dst_ch == 1 {
        let g = 1.0 / src_ch as f32;
        for f in 0..DSP_BLOCK_FRAMES {
            let sb = f * src_ch;
            let mut acc = 0.0;
            for c in 0..src_ch {
                acc += src[sb + c];
            }
            dst[f] += acc * g;
        }
        return;
    }
    if src_ch == 1 {
        // Mono upmix: a single-channel source feeds every destination channel.
        for f in 0..DSP_BLOCK_FRAMES {
            let v = src[f];
            let db = f * dst_ch;
            for c in 0..dst_ch {
                dst[db + c] += v;
            }
        }
        return;
    }
    let n = src_ch.min(dst_ch);
    for f in 0..DSP_BLOCK_FRAMES {
        let sb = f * src_ch;
        let db = f * dst_ch;
        for c in 0..n {
            dst[db + c] += src[sb + c];
        }
    }
}

/// Add `src` (downmixed to mono) into a single physical channel `ch` of `dst`.
#[inline]
fn add_to_channel(src: &[f32], dst: &mut [f32], ch: usize) {
    let src_ch = src.len() / DSP_BLOCK_FRAMES;
    let dst_ch = dst.len() / DSP_BLOCK_FRAMES;
    if src_ch == 0 || ch >= dst_ch {
        return;
    }
    let g = 1.0 / src_ch as f32;
    for f in 0..DSP_BLOCK_FRAMES {
        let sb = f * src_ch;
        let mut acc = 0.0;
        for c in 0..src_ch {
            acc += src[sb + c];
        }
        dst[f * dst_ch + ch] += acc * g;
    }
}

/// Place `src`'s channels into `dst` starting at channel `off`. Distinct offsets
/// leave inputs side by side; `dst` is zeroed each block so this is a copy.
#[inline]
fn add_block_at(src: &[f32], dst: &mut [f32], off: usize) {
    let src_ch = src.len() / DSP_BLOCK_FRAMES;
    let dst_ch = dst.len() / DSP_BLOCK_FRAMES;
    if src_ch == 0 || off >= dst_ch {
        return;
    }
    let n = src_ch.min(dst_ch - off);
    for f in 0..DSP_BLOCK_FRAMES {
        let sb = f * src_ch;
        let db = f * dst_ch + off;
        for c in 0..n {
            dst[db + c] += src[sb + c];
        }
    }
}

struct DelayLine {
    buf: Box<[f32]>,
    pos: usize,
    channels: usize,
}

impl DelayLine {
    fn new(delay_frames: usize, channels: usize) -> Self {
        Self {
            buf: vec![0.0; delay_frames * channels].into_boxed_slice(),
            pos: 0,
            channels,
        }
    }

    fn process_and_add(&mut self, input: &[f32], dst: &mut [f32]) {
        let cap = self.buf.len();
        if cap == 0 {
            add_mapped(input, dst);
            return;
        }
        let src_ch = self.channels;
        let dst_ch = dst.len() / DSP_BLOCK_FRAMES;
        let n = src_ch.min(dst_ch);
        let frames = input.len() / src_ch;
        let mut pos = self.pos;
        for f in 0..frames {
            for c in 0..src_ch {
                let delayed = self.buf[pos];
                self.buf[pos] = input[f * src_ch + c];
                if c < n {
                    dst[f * dst_ch + c] += delayed;
                }
                pos = if pos + 1 == cap { 0 } else { pos + 1 };
            }
        }
        self.pos = pos;
    }
}

/// Per-output DAG runtime: sources + effects in topological order plus the
/// terminal edges whose buffers get summed into the final output.
pub(super) struct OutputGraph {
    sample_rate: u32,
    /// Interleaved channel width of `process_block`'s output. Stereo unless a
    /// speaker sets it to the device's channel count.
    out_channels: usize,
    nodes: Vec<DagNode>,
    terminals: Vec<TerminalEdge>,
}

impl OutputGraph {
    pub(super) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(super) fn out_channels(&self) -> usize {
        self.out_channels
    }

    pub(super) fn set_out_channels(&mut self, channels: usize) {
        self.out_channels = channels;
    }

    /// True if every source has enough buffered input to produce one full
    /// output block without underrun. Availability-paced workers use this to
    /// gate block production.
    pub(super) fn all_sources_ready(&self) -> bool {
        for node in &self.nodes {
            match node {
                DagNode::Source(s) if !s.is_ready_for_block() => return false,
                // A network producer paces an availability worker (file
                // recording) at the real-time arrival rate; without this the
                // recorder spins flat-out and writes minutes per second.
                DagNode::Producer(p) if !p.is_ready_for_block() => return false,
                _ => {}
            }
        }
        true
    }

    /// Fill `output` (`DSP_BLOCK_FRAMES * out_channels` long) with one block of
    /// mixed audio at `sample_rate`.
    pub(super) fn process_block(&mut self, output: &mut [f32]) {
        for node in &mut self.nodes {
            match node {
                DagNode::Source(s) => s.fill_block(),
                DagNode::Producer(p) => p.process(),
                DagNode::Effect(_) | DagNode::Consumer(_) => {}
            }
        }
        // `split_at_mut` gives mutable access to effect `i` while keeping
        // immutable access to its upstreams (all at indices < i by topo sort).
        for i in 0..self.nodes.len() {
            let (head, tail) = self.nodes.split_at_mut(i);
            if let DagNode::Consumer(cons) = &mut tail[0] {
                for (_, buf) in cons.channel_bufs.iter_mut() {
                    for s in buf.iter_mut() {
                        *s = 0.0;
                    }
                }
                for edge in &mut cons.incoming {
                    let src = head[edge.src_idx].out_buf_for_handle(edge.source_handle.as_deref());
                    let Some((_, buf)) = cons
                        .channel_bufs
                        .iter_mut()
                        .find(|(h, _)| Some(h.as_str()) == edge.target_handle.as_deref())
                    else {
                        continue;
                    };
                    match &mut edge.delay {
                        Some(d) => d.process_and_add(src, buf),
                        None => add_mapped(src, buf),
                    }
                }
                for (i, (_, buf)) in cons.channel_bufs.iter().enumerate() {
                    if let Some(prod) = cons.send_producers.get_mut(i) {
                        bulk_push(prod, buf);
                    }
                }
                continue;
            }
            if let DagNode::Effect(eff) = &mut tail[0] {
                // WebRTC bridge: sum each incoming edge into its channel input
                // buffer by target handle, then push to the send rings.
                if !eff.channel_bufs.is_empty() {
                    for (_, buf) in eff.channel_bufs.iter_mut() {
                        for s in buf.iter_mut() {
                            *s = 0.0;
                        }
                    }
                    for edge in &mut eff.incoming {
                        let src =
                            head[edge.src_idx].out_buf_for_handle(edge.source_handle.as_deref());
                        let Some((_, buf)) = eff
                            .channel_bufs
                            .iter_mut()
                            .find(|(h, _)| Some(h.as_str()) == edge.target_handle.as_deref())
                        else {
                            continue;
                        };
                        match &mut edge.delay {
                            Some(d) => d.process_and_add(src, buf),
                            None => add_mapped(src, buf),
                        }
                    }
                    eff.effects[0].push_channel_inputs(&eff.channel_bufs);
                    // out_buf becomes the global mix (every peer, every channel).
                    eff.effects[0]
                        .process_with_sidechain(&mut eff.out_buf, None, DSP_BLOCK_FRAMES);
                    eff.effects[0]
                        .populate_handle_bufs(&mut eff.handle_bufs, DSP_BLOCK_FRAMES);
                    continue;
                }
                for s in eff.out_buf.iter_mut() {
                    *s = 0.0;
                }
                for edge in &mut eff.incoming {
                    let src = head[edge.src_idx].out_buf_for_handle(edge.source_handle.as_deref());
                    let route = edge.target_handle.as_deref().and_then(target_route);
                    match (edge.stack_ch, &mut edge.delay, route) {
                        (Some(off), _, _) => add_block_at(src, &mut eff.out_buf, off),
                        (None, Some(d), _) => d.process_and_add(src, &mut eff.out_buf),
                        (None, None, Some((off, 1))) => add_to_channel(src, &mut eff.out_buf, off),
                        (None, None, Some((off, _))) => add_block_at(src, &mut eff.out_buf, off),
                        (None, None, None) => add_mapped(src, &mut eff.out_buf),
                    }
                }
                if let Some(sc_buf) = eff.sidechain_buf.as_mut() {
                    for s in sc_buf.iter_mut() {
                        *s = 0.0;
                    }
                    for edge in &mut eff.sidechain {
                        let src =
                            head[edge.src_idx].out_buf_for_handle(edge.source_handle.as_deref());
                        match &mut edge.delay {
                            Some(d) => d.process_and_add(src, sc_buf),
                            None => add_mapped(src, sc_buf),
                        }
                    }
                }
                if !eff.bypass.load(Ordering::Relaxed) {
                    eff.run(DSP_BLOCK_FRAMES);
                }
                eff.effects[0]
                    .populate_handle_bufs(&mut eff.handle_bufs, DSP_BLOCK_FRAMES);
                let w = eff.out_buf.len() / DSP_BLOCK_FRAMES;
                for (h, buf) in eff.handle_bufs.iter_mut() {
                    if let Some(a) = parse_stereo(h) {
                        let c0 = (a - 1).min(w - 1);
                        let c1 = a.min(w - 1);
                        for f in 0..DSP_BLOCK_FRAMES {
                            buf[f * 2] = eff.out_buf[f * w + c0];
                            buf[f * 2 + 1] = eff.out_buf[f * w + c1];
                        }
                    } else if let Some(k) = parse_ch(h) {
                        let c = (k - 1).min(w - 1);
                        for f in 0..DSP_BLOCK_FRAMES {
                            buf[f] = eff.out_buf[f * w + c];
                        }
                    }
                }
            }
        }
        for s in output.iter_mut() {
            *s = 0.0;
        }
        for terminal in &mut self.terminals {
            let src =
                self.nodes[terminal.src_idx].out_buf_for_handle(terminal.source_handle.as_deref());
            match (&mut terminal.delay, terminal.route) {
                (Some(d), _) => d.process_and_add(src, output),
                (None, Some((off, 1))) => add_to_channel(src, output, off),
                (None, Some((off, _))) => add_block_at(src, output, off),
                (None, None) => add_mapped(src, output),
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
    realtime: bool,
    valid: &ValidGraph,
    input_native_sr: &HashMap<String, u32>,
    input_native_channels: &HashMap<String, u32>,
    producer_pairs: &mut Vec<(String, Producer<f32>)>,
    registry: &mut EffectRegistry,
    input_volumes: &HashMap<String, Arc<AtomicU32>>,
    input_paused: &HashMap<String, Arc<AtomicBool>>,
    input_drain: &HashMap<String, Arc<AtomicU64>>,
    input_meters: &HashMap<String, MeterHandle>,
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
    // Per-node channel width; effects inherit the max width of their upstreams.
    let mut node_channels: Vec<usize> = Vec::with_capacity(topo.len());

    for id in &topo {
        if let Some(input) = valid.inputs.iter().find(|i| &i.id == id) {
            // NetReceiver is a network producer, not a captured source: it emits
            // per-channel outputs from a shared jitter buffer at the output rate.
            if let InputSpec::NetReceiver { port } = input.spec {
                let receiver = crate::audio::netaudio::receiver::get_or_create(id, port);
                let receiver =
                    ChannelReceiver::new(receiver.register_consumer(output_sr, realtime));
                let mut handles: Vec<String> = valid
                    .edges
                    .iter()
                    .filter(|e| &e.from == id)
                    .filter_map(|e| e.source_handle.clone())
                    .collect();
                handles.sort();
                handles.dedup();
                let pw = 2;
                let handle_bufs = handles
                    .into_iter()
                    .map(|h| (h, vec![0.0; DSP_BLOCK_FRAMES * pw]))
                    .collect();
                id_to_index.insert(id.clone(), nodes.len());
                nodes.push(DagNode::Producer(ProducerState {
                    receiver,
                    out_buf: vec![0.0; DSP_BLOCK_FRAMES * pw],
                    handle_bufs,
                }));
                node_latencies.push(0);
                node_channels.push(pw);
                continue;
            }
            // File sources are paced by backpressure; dropping backlog plays fast.
            let source_realtime =
                realtime && !matches!(input.spec, InputSpec::AudioFile { .. });
            let input_sr = *input_native_sr
                .get(id)
                .ok_or_else(|| AppError::Validation(format!("input {id} has no SR")))?;
            let (producer, consumer) = RingBuffer::<f32>::new(RING_CAPACITY);
            producer_pairs.push((id.clone(), producer));

            let source_channels = input_native_channels.get(id).copied().unwrap_or(2) as usize;
            let mut ch_handles: Vec<String> = valid
                .edges
                .iter()
                .filter(|e| &e.from == id)
                .filter_map(|e| e.source_handle.clone())
                .filter(|h| tap_handle_width(h).is_some())
                .collect();
            ch_handles.sort();
            ch_handles.dedup();
            let source_handle_bufs: Vec<(String, Vec<f32>)> = ch_handles
                .into_iter()
                .map(|h| {
                    let w = tap_handle_width(&h).unwrap_or(1);
                    (h, vec![0.0; DSP_BLOCK_FRAMES * w])
                })
                .collect();
            let resampler = if input_sr == output_sr {
                None
            } else {
                Some(MultiResampler::new(input_sr, output_sr, RESAMPLE_CHUNK, source_channels)?)
            };
            let out_max = resampler.as_ref().map(|r| r.out_max()).unwrap_or(RESAMPLE_CHUNK);
            // x4 headroom: one chunk draining + one in-flight + alignment slack.
            let staging_cap = out_max * 4 + DSP_BLOCK_FRAMES * source_channels;
            let input_frames_per_block = (DSP_BLOCK_FRAMES as u64 * input_sr as u64
                + output_sr as u64
                - 1)
                / output_sr as u64;
            let input_samples_per_block = (input_frames_per_block as usize) * source_channels;

            let source = SourceState {
                label: format!("{id}@{input_sr}->{output_sr}"),
                channels: source_channels,
                consumer,
                resampler,
                input_staging: Vec::with_capacity(RESAMPLE_CHUNK * source_channels + 8),
                out_pending: StagingRing::with_capacity(staging_cap),
                chunk_tmp: Vec::with_capacity(out_max * source_channels),
                out_buf: vec![0.0; DSP_BLOCK_FRAMES * source_channels],
                input_samples_per_block,
                realtime: source_realtime,
                last_pop_at: Instant::now(),
                first_data_logged: false,
                volume: input_volumes
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits()))),
                paused: input_paused.get(id).cloned(),
                drain: input_drain.get(id).cloned(),
                last_drain_gen: 0,
                meter: input_meters.get(id).cloned(),
                handle_bufs: source_handle_bufs,
            };
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Source(source));
            node_latencies.push(0);
            node_channels.push(source_channels);
        } else if let Some(effect) = valid.effects.iter().find(|e| &e.id == id) {
            let build = instantiate_effect(&effect.spec, id, output_sr, realtime, registry);
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
            type Upstream = (usize, Option<String>, Option<String>);
            let mut main_upstream: Vec<Upstream> = Vec::new();
            let mut side_upstream: Vec<Upstream> = Vec::new();
            for e in &valid.edges {
                if &e.to == id && reachable.contains(&e.from) {
                    let idx = id_to_index[&e.from];
                    let entry = (idx, e.source_handle.clone(), e.target_handle.clone());
                    match e.kind {
                        EdgeKind::Main => main_upstream.push(entry),
                        EdgeKind::Sidechain => side_upstream.push(entry),
                    }
                }
            }
            let max_upstream = main_upstream
                .iter()
                .chain(side_upstream.iter())
                .map(|(i, _, _)| node_latencies[*i])
                .max()
                .unwrap_or(0);
            // Width is the max of: upstream widths, any `chK` target channel fed
            // in, and any `chK` output tap drawn off this effect.
            // A chK source handle carries exactly its tapped channel (mono), so
            // the edge width is the tap buffer's, not the source node's full width.
            let upstream_w = main_upstream
                .iter()
                .map(|(i, sh, _)| match sh.as_deref() {
                    Some(h) if tap_handle_width(h).is_some() => {
                        nodes[*i].out_buf_for_handle(Some(h)).len() / DSP_BLOCK_FRAMES
                    }
                    _ => node_channels[*i],
                })
                .max()
                .unwrap_or(2);
            let target_w = main_upstream
                .iter()
                .filter_map(|(_, _, t)| t.as_deref().and_then(target_route))
                .map(|(off, w)| off + w)
                .max()
                .unwrap_or(0);
            let tap_w = valid
                .edges
                .iter()
                .filter(|e| &e.from == id)
                .filter_map(|e| e.source_handle.as_deref())
                .filter_map(|h| parse_stereo(h).map(|a| a + 1).or_else(|| parse_ch(h)))
                .max()
                .unwrap_or(0);
            // Monitors in `mix` mode stack each input onto its own channel; in
            // `split` mode they route each `chK` input to channel k like any
            // other per-channel node. Everything else sums its inputs by channel.
            let (is_analyzer, split, declared_ch) = match &effect.spec {
                EffectSpec::LevelMeter(d) => {
                    (true, d.channels_expanded, d.channels as usize)
                }
                EffectSpec::Waveform(d) => (true, d.channels_expanded, d.channels as usize),
                _ => (false, false, 0),
            };
            let stack = is_analyzer && !split;
            let edge_w = |i: usize, sh: &Option<String>| -> usize {
                match sh.as_deref() {
                    Some(h) if tap_handle_width(h).is_some() => {
                        nodes[i].out_buf_for_handle(Some(h)).len() / DSP_BLOCK_FRAMES
                    }
                    _ => node_channels[i],
                }
            };
            let eff_channels = if stack {
                main_upstream
                    .iter()
                    .map(|(i, s, _)| edge_w(*i, s))
                    .sum::<usize>()
                    .max(1)
            } else if split {
                target_w.max(declared_ch).max(1)
            } else {
                upstream_w.max(target_w).max(tap_w).max(1)
            };
            let make_edge = |src_idx: usize,
                             source_handle: Option<String>,
                             target_handle: Option<String>,
                             stack_ch: Option<usize>| {
                let pad = max_upstream - node_latencies[src_idx];
                IncomingEdge {
                    src_idx,
                    source_handle,
                    target_handle,
                    stack_ch,
                    delay: if pad > 0 {
                        Some(DelayLine::new(pad, node_channels[src_idx]))
                    } else {
                        None
                    },
                }
            };
            let mut stack_off = 0;
            let incoming: Vec<IncomingEdge> = main_upstream
                .into_iter()
                .map(|(i, s, t)| {
                    let stack_at = if stack {
                        let cur = stack_off;
                        stack_off += edge_w(i, &s);
                        Some(cur)
                    } else {
                        None
                    };
                    make_edge(i, s, t, stack_at)
                })
                .collect();
            let sidechain: Vec<IncomingEdge> = side_upstream
                .into_iter()
                .map(|(i, s, t)| make_edge(i, s, t, None))
                .collect();
            let sidechain_buf = if sidechain.is_empty() {
                None
            } else {
                Some(vec![0.0; DSP_BLOCK_FRAMES * eff_channels])
            };
            // Output taps drawn off this effect: WebRTC `peer:<id>` mixes plus
            // generic `chK` per-channel taps. A stale handle just yields silence.
            let is_webrtc = matches!(effect.spec, EffectSpec::WebRtcBridge { .. });
            let mut handle_ids: Vec<String> = valid
                .edges
                .iter()
                .filter(|e| &e.from == id)
                .filter_map(|e| e.source_handle.clone())
                .filter(|h| tap_handle_width(h).is_some() || (is_webrtc && h.starts_with("peer:")))
                .collect();
            handle_ids.sort();
            handle_ids.dedup();
            let handle_bufs: Vec<(String, Vec<f32>)> = handle_ids
                .into_iter()
                .map(|h| {
                    let w = tap_handle_width(&h).unwrap_or(2);
                    (h, vec![0.0; DSP_BLOCK_FRAMES * w])
                })
                .collect();
            // One input buffer per WebRTC send channel, keyed "ch1".."chN".
            let channel_bufs: Vec<(String, Vec<f32>)> =
                if let EffectSpec::WebRtcBridge { channels, .. } = &effect.spec {
                    (1..=(*channels).clamp(1, 10))
                        .map(|c| (format!("ch{c}"), vec![0.0; DSP_BLOCK_FRAMES * 2]))
                        .collect()
                } else {
                    Vec::new()
                };
            // Analyzers read all channels at once; WebRTC bridge is single.
            // Everything else runs one instance per stereo pair.
            let full_width = matches!(
                effect.spec,
                EffectSpec::LevelMeter(_) | EffectSpec::Waveform(_)
            );
            let pairs = if full_width || matches!(effect.spec, EffectSpec::WebRtcBridge { .. }) {
                1
            } else {
                eff_channels.div_ceil(2)
            };
            let mut effects = Vec::with_capacity(pairs);
            let own = build.effect.latency_frames();
            effects.push(build.effect);
            for _ in 1..pairs {
                let extra = instantiate_effect(&effect.spec, id, output_sr, realtime, registry);
                effects.push(extra.effect);
            }
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Effect(EffectState {
                effects,
                full_width,
                bypass,
                incoming,
                sidechain,
                out_buf: vec![0.0; DSP_BLOCK_FRAMES * eff_channels],
                sidechain_buf,
                pair_main: vec![0.0; DSP_BLOCK_FRAMES * 2],
                pair_side: vec![0.0; DSP_BLOCK_FRAMES * 2],
                handle_bufs,
                channel_bufs,
            }));
            node_latencies.push(max_upstream + own);
            node_channels.push(eff_channels);
        }
    }

    // A NetSender output is a terminal Consumer node inside the DAG (not a
    // summed output terminal): it sums per-channel inputs and pushes them into
    // send rings drained by the background UDP transmitter.
    let net_sender = output_id
        .and_then(|oid| valid.outputs.iter().find(|o| o.id == oid))
        .and_then(|o| match &o.spec {
            OutputSpec::NetSender { .. } => Some(o.spec.clone()),
            _ => None,
        });
    if let Some(OutputSpec::NetSender {
        node_id,
        target,
        channels,
        codec,
        opus_bitrate,
        opus_application,
    }) = net_sender
    {
        let oid = output_id.unwrap();
        let mut up: Vec<(usize, Option<String>, Option<String>)> = Vec::new();
        for e in &valid.edges {
            if e.to == oid && reachable.contains(&e.from) {
                let idx = id_to_index[&e.from];
                up.push((idx, e.source_handle.clone(), e.target_handle.clone()));
            }
        }
        let max_up = up.iter().map(|(i, _, _)| node_latencies[*i]).max().unwrap_or(0);
        let incoming: Vec<IncomingEdge> = up
            .into_iter()
            .map(|(idx, source_handle, target_handle)| {
                let pad = max_up - node_latencies[idx];
                IncomingEdge {
                    src_idx: idx,
                    source_handle,
                    target_handle,
                    stack_ch: None,
                    delay: if pad > 0 {
                        Some(DelayLine::new(pad, node_channels[idx]))
                    } else {
                        None
                    },
                }
            })
            .collect();

        let n = channels.clamp(1, 10) as usize;
        let mut channel_bufs: Vec<(String, Vec<f32>)> = Vec::with_capacity(n);
        let mut send_producers: Vec<Producer<f32>> = Vec::with_capacity(n);
        let mut send_consumers: Vec<Consumer<f32>> = Vec::with_capacity(n);
        for c in 1..=n {
            channel_bufs.push((format!("ch{c}"), vec![0.0; DSP_BLOCK_FRAMES * 2]));
            let (prod, cons) = RingBuffer::<f32>::new(crate::audio::netaudio::SEND_RING);
            send_producers.push(prod);
            send_consumers.push(cons);
        }
        let format = match codec {
            NetCodec::PcmF32 => Format::PcmF32,
            NetCodec::PcmI16 => Format::PcmI16,
            NetCodec::Opus => Format::Opus,
        };
        let sender = crate::audio::netaudio::sender::get_or_create(
            &node_id,
            target,
            format,
            opus_bitrate,
            opus_application,
        );
        sender.set_send_consumers(send_consumers);

        nodes.push(DagNode::Consumer(ConsumerState {
            incoming,
            channel_bufs,
            send_producers,
        }));

        return Ok(BuiltOutputGraph {
            graph: OutputGraph {
                sample_rate: output_sr,
                out_channels: 2,
                nodes,
                terminals: Vec::new(),
            },
            controls,
            bypasses,
            meters,
            lufs,
            gr_handles,
            scopes,
        });
    }

    let terminals: Vec<TerminalEdge> = match output_id {
        Some(id) => {
            let upstream: Vec<(usize, Option<String>, Option<(usize, usize)>)> = valid
                .edges
                .iter()
                .filter(|e| e.to == id)
                .filter_map(|e| {
                    id_to_index.get(&e.from).copied().map(|idx| {
                        let route = e.target_handle.as_deref().and_then(target_route);
                        (idx, e.source_handle.clone(), route)
                    })
                })
                .collect();
            let max_upstream = upstream
                .iter()
                .map(|(i, _, _)| node_latencies[*i])
                .max()
                .unwrap_or(0);
            upstream
                .into_iter()
                .map(|(src_idx, source_handle, route)| {
                    let pad = max_upstream - node_latencies[src_idx];
                    TerminalEdge {
                        src_idx,
                        source_handle,
                        route,
                        delay: if pad > 0 {
                            Some(DelayLine::new(pad, node_channels[src_idx]))
                        } else {
                            None
                        },
                    }
                })
                .collect()
        }
        None => Vec::new(),
    };

    Ok(BuiltOutputGraph {
        graph: OutputGraph {
            sample_rate: output_sr,
            out_channels: 2,
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

#[cfg(test)]
mod tests {
    use super::{add_mapped, DSP_BLOCK_FRAMES};

    #[test]
    fn add_mapped_maps_by_channel() {
        // 4->2: first two channels pass, rest dropped.
        let mut src = vec![0.0; DSP_BLOCK_FRAMES * 4];
        for f in 0..DSP_BLOCK_FRAMES {
            for c in 0..4 {
                src[f * 4 + c] = c as f32 + 1.0;
            }
        }
        let mut dst = vec![0.0; DSP_BLOCK_FRAMES * 2];
        add_mapped(&src, &mut dst);
        assert_eq!(dst[0], 1.0);
        assert_eq!(dst[1], 2.0);

        // 2->1: mono downmix is the mean.
        let mut stereo = vec![0.0; DSP_BLOCK_FRAMES * 2];
        for f in 0..DSP_BLOCK_FRAMES {
            stereo[f * 2] = 1.0;
            stereo[f * 2 + 1] = 3.0;
        }
        let mut mono = vec![0.0; DSP_BLOCK_FRAMES];
        add_mapped(&stereo, &mut mono);
        assert!((mono[0] - 2.0).abs() < 1e-6);

        // equal width: straight sum-in.
        let src = vec![0.5; DSP_BLOCK_FRAMES * 3];
        let mut dst = vec![0.25; DSP_BLOCK_FRAMES * 3];
        add_mapped(&src, &mut dst);
        assert!((dst[0] - 0.75).abs() < 1e-6);
    }
}
