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
use crate::audio::health;
use crate::audio::netaudio::packet::Format;
use crate::audio::resample::MultiResampler;
use crate::audio::stream_recv::ChannelReceiver;
use crate::audio::input_bridge::CaptureStats;
use crate::audio::streams::bulk_push_counted;
use crate::error::{AppError, AppResult};

/// Ring buffer length in frames per source; multiplied by the source's channel
/// count at build time. ~500 ms at 96 kHz so the worker rides out longer source
/// pauses (SCK silent gaps, scheduler hiccups, capture-clock drift) without
/// overflowing the FAST source's ring while waiting on a SLOW one.
pub(super) const RING_CAPACITY_FRAMES: usize = 48_000;

/// Block size used by the resampler. 256 frames @ 48 kHz ~ 5.3 ms.
pub(super) const RESAMPLE_CHUNK: usize = 256;

pub const DSP_BLOCK_FRAMES: usize = 1024;

const MAX_NET_CH: u32 = crate::audio::netaudio::MAX_CHANNELS as u32;

/// How long a source can go without delivering before the availability-paced
/// worker stops waiting on it. SCK in normal operation delivers every ~20 ms,
/// so 150 ms is ~7x headroom -- enough to avoid false positives on bursty
/// delivery, short enough that a real stall doesn't drown the FAST source's
/// ring buffer.
const STALL_THRESHOLD: Duration = Duration::from_millis(150);

const SOURCE_BACKLOG_HIGH_BLOCKS: usize = 4;
const SOURCE_BACKLOG_LOW_BLOCKS: usize = 2;
/// Ceiling on one block's trim. Backlog drains over a few seconds instead of
/// vanishing in a single splice, which is what makes it inaudible.
const TRIM_MAX_FRAMES_PER_BLOCK: usize = 64;
/// Crossfade length across a trim's cut. Long enough to kill the step, short
/// enough that the replayed audio reads as texture rather than an echo.
const SPLICE_FADE_FRAMES: usize = 32;

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
        let overrun = (src.len() - take) as u64;
        self.dropped = self.dropped.saturating_add(overrun);
        health::bump(&health::STAGING_OVERRUN_SAMPLES, overrun);
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

/// Per-source counters + gauge read by the non-RT tick thread (`meter::spawn_xrun_thread`).
/// Every write is `Ordering::Relaxed` -- RT-safe, no allocation, no other sync.
#[derive(Clone)]
pub(super) struct SourceStats {
    /// Samples zero-filled on genuine mid-stream underrun (ring ran dry while streaming).
    pub xrun: Arc<AtomicU64>,
    /// Samples silenced because the source delivered nothing for longer than
    /// `STALL_THRESHOLD`. Silent by design, but it is still missing audio.
    pub stalled: Arc<AtomicU64>,
    /// Samples discarded by the backlog trim in `fill_block`.
    pub trimmed: Arc<AtomicU64>,
    /// Samples actually read out of the source ring.
    pub consumed: Arc<AtomicU64>,
    /// Ring occupancy (samples) at the end of the last `fill_block`. A gauge,
    /// not a counter -- plain `store`, no accumulation.
    pub level: Arc<AtomicU64>,
}

impl SourceStats {
    fn new() -> Self {
        Self {
            xrun: Arc::new(AtomicU64::new(0)),
            stalled: Arc::new(AtomicU64::new(0)),
            trimmed: Arc::new(AtomicU64::new(0)),
            consumed: Arc::new(AtomicU64::new(0)),
            level: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Identifies one source for the tick thread: its counters plus enough
/// context (channel count, native rate) to convert sample deltas into a frame
/// rate comparable against real time.
#[derive(Clone)]
pub(super) struct SourceMeta {
    pub label: String,
    pub stats: SourceStats,
    pub channels: usize,
    pub native_sr: u32,
    /// Native-rate frames this source consumes per block. The rate check needs
    /// it as the counter's step size, since a window boundary can misattribute
    /// a whole block.
    pub frames_per_block: usize,
    /// Graph id of the captured input this source reads, for matching against
    /// the broadcast slot's `CaptureStats` once the bridge wires it up. `None`
    /// for ring-sources and network producers -- they don't go through a
    /// capture broadcast.
    pub input_id: Option<String>,
    /// Owning output id (or "monitor"), the other half of the key that
    /// disambiguates one input feeding several outputs.
    pub output_id: String,
    /// Capture-side fed/dropped counters, filled in by `pipeline/mod.rs`
    /// after `BroadcastTx::add` returns them for this source's ring.
    pub capture: Option<CaptureStats>,
}

/// Identifies one output for the tick thread: its per-block counter plus the
/// sample rate that defines its expected block cadence. `channels` and `io`
/// are only meaningful for speaker outputs -- `build_output_graph` doesn't
/// know the device's real channel count yet, so the caller fills both in
/// after `start_speaker_stream` returns (see `pipeline/mod.rs`).
#[derive(Clone)]
pub(super) struct OutputMeta {
    pub label: String,
    pub blocks: Arc<AtomicU64>,
    pub sample_rate: u32,
    pub channels: usize,
    pub io: Option<super::output::SpeakerIo>,
}

struct SourceState {
    label: String,
    channels: usize,
    consumer: Consumer<f32>,
    resampler: Option<MultiResampler>,
    input_staging: Vec<f32>,
    /// Holds a trim's crossfaded join until the refill path picks it up.
    splice_tmp: Vec<f32>,
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
    // Per-channel taps ("chK") drawn off this source.
    handle_bufs: Vec<(String, Vec<f32>)>,
    stats: SourceStats,
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

    /// Taps are filled at the end of `fill_block`, so an early return would
    /// leave them looping their last block -- a buzz at the block rate.
    fn silence(&mut self) {
        self.out_buf.fill(0.0);
        for (_, buf) in self.handle_bufs.iter_mut() {
            buf.fill(0.0);
        }
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
                self.silence();
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
                self.silence();
                return;
            }
        }
        // Trim input backlog toward LOW so latency stays bounded, a slice per
        // block and spliced rather than cut: drift needs a trickle, and one
        // discard of hundreds of milliseconds is an audible tear.
        if self.realtime {
            let have = self.consumer.slots();
            let high = self.input_samples_per_block * SOURCE_BACKLOG_HIGH_BLOCKS;
            if have > high {
                let low = self.input_samples_per_block * SOURCE_BACKLOG_LOW_BLOCKS;
                let fade = SPLICE_FADE_FRAMES * self.channels;
                let budget = TRIM_MAX_FRAMES_PER_BLOCK * self.channels;
                let excess = (have - low).min(budget);
                let drop = excess - excess % self.channels;
                // The splice reads a fade-out and a fade-in around the cut, so
                // the ring has to hold both on top of what it discards.
                if drop > 0 && have >= drop + 2 * fade {
                    self.splice_trim(drop, fade);
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
                // A stalled/paused source silences by design; only a source that
                // is actively streaming and ran dry mid-block is a real xrun.
                let counter = if self.is_stalled() {
                    &self.stats.stalled
                } else {
                    &self.stats.xrun
                };
                counter.fetch_add((need - written) as u64, Ordering::Relaxed);
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
        self.stats
            .level
            .store(self.consumer.slots() as u64, Ordering::Relaxed);
    }

    /// Removes `drop` samples from the input ring, crossfading the `fade`
    /// samples before the cut into the `fade` after it. The joined slice leads
    /// the stream through `input_staging`, so the listener hears one short
    /// blend instead of a step.
    fn splice_trim(&mut self, drop: usize, fade: usize) {
        self.splice_tmp.clear();
        let Ok(outgoing) = self.consumer.read_chunk(fade) else {
            return;
        };
        let (first, second) = outgoing.as_slices();
        self.splice_tmp.extend_from_slice(first);
        self.splice_tmp.extend_from_slice(second);
        outgoing.commit_all();

        if let Ok(cut) = self.consumer.read_chunk(drop) {
            cut.commit_all();
        }

        if let Ok(incoming) = self.consumer.read_chunk(fade) {
            let (first, second) = incoming.as_slices();
            crossfade_into(&mut self.splice_tmp, first, second, self.channels);
            incoming.commit_all();
        }

        self.input_staging.extend_from_slice(&self.splice_tmp);
        // What left the ring, versus what the stream actually loses: the
        // fade-out is re-injected, so only the cut and the fade-in are gone.
        let popped = (drop + 2 * fade) as u64;
        let removed = (drop + fade) as u64;
        self.stats.consumed.fetch_add(popped, Ordering::Relaxed);
        self.stats.trimmed.fetch_add(removed, Ordering::Relaxed);
        health::bump(&health::SOURCE_TRIM_DROPPED_SAMPLES, removed);
        self.last_pop_at = Instant::now();
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
                    self.stats.consumed.fetch_add(avail as u64, Ordering::Relaxed);
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
            let mut want = RESAMPLE_CHUNK * self.channels;
            // A splice staged its joined frames ahead of the ring.
            if !self.input_staging.is_empty() {
                let n = self.input_staging.len().min(want);
                self.chunk_tmp.extend_from_slice(&self.input_staging[..n]);
                self.input_staging.drain(..n);
                want -= n;
            }
            let avail = self.consumer.slots().min(want);
            if avail > 0 {
                if let Ok(chunk) = self.consumer.read_chunk(avail) {
                    let (first, second) = chunk.as_slices();
                    self.chunk_tmp.extend_from_slice(first);
                    self.chunk_tmp.extend_from_slice(second);
                    chunk.commit_all();
                    self.stats.consumed.fetch_add(avail as u64, Ordering::Relaxed);
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
    // When this node fans out to several outputs it is computed once (here, in
    // its owning output's graph) and its `out_buf` is published each block into
    // one ring per other consuming output, which reads it via a ring-source.
    taps: Vec<Producer<f32>>,
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
    /// What each `handle_bufs` entry draws from the tap map, which is keyed by
    /// what the sender stamped rather than by the handle the UI draws.
    wire_keys: Vec<TapKey>,
}

/// A handle either reads one tap or sums a group of them: a WebRTC peer's mix
/// is every channel that peer sends, and the peer is the key prefix.
enum TapKey {
    Channel(String),
    PrefixMix(String),
}

impl ProducerState {
    fn process(&mut self) {
        self.receiver.mix_block(&mut self.out_buf);
        for ((_, buf), key) in self.handle_bufs.iter_mut().zip(&self.wire_keys) {
            match key {
                TapKey::Channel(k) => self.receiver.channel(k, buf),
                TapKey::PrefixMix(p) => self.receiver.prefix_mix(p, buf),
            }
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

/// What a network producer's source handle reads from the tap map. `chN` is the
/// direct-IP wire index (0-based on the wire, 1-based in the UI); `peer:<id>:<ch>`
/// is a WebRTC tap key verbatim, and `peer:<id>` sums that peer's channels.
fn tap_key(handle: &str) -> Option<TapKey> {
    if let Some(rest) = handle.strip_prefix("peer:") {
        return Some(if rest.contains(':') {
            TapKey::Channel(rest.to_string())
        } else {
            TapKey::PrefixMix(format!("{rest}:"))
        });
    }
    parse_ch(handle).map(|ch| TapKey::Channel((ch - 1).to_string()))
}

/// Parse an `stA` stereo-group handle into its 1-based lower channel; the group
/// carries channels A and A+1.
#[inline]
fn parse_stereo(handle: &str) -> Option<usize> {
    handle.strip_prefix("st").and_then(|s| s.parse::<usize>().ok())
}

/// Channel width an edge actually carries. A `chK`/`stA` source handle taps a
/// slice of its node, so the node's own width would be wrong.
fn edge_channels(
    nodes: &[DagNode],
    node_channels: &[usize],
    idx: usize,
    source_handle: Option<&str>,
) -> usize {
    match source_handle {
        Some(h) if tap_handle_width(h).is_some() => {
            nodes[idx].out_buf_for_handle(Some(h)).len() / DSP_BLOCK_FRAMES
        }
        _ => node_channels[idx],
    }
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

/// Pure time shift. It deliberately does no channel mapping: the caller adds
/// the shifted block through the same `add_*` path an undelayed edge takes, so
/// routing, upmix and downmix cannot drift between the two.
/// Blends `dst` (the audio before a trim's cut) into the audio after it, given
/// as a ring's two halves. Both ends stay continuous: frame 0 is pure `dst` and
/// the last frame is pure incoming, so neither join is a step.
fn crossfade_into(dst: &mut [f32], first: &[f32], second: &[f32], channels: usize) {
    let span = (SPLICE_FADE_FRAMES - 1).max(1) as f32;
    for (i, s) in first.iter().chain(second.iter()).enumerate() {
        if i >= dst.len() {
            break;
        }
        let w = ((i / channels) as f32 / span).min(1.0);
        dst[i] = dst[i] * (1.0 - w) + s * w;
    }
}

struct DelayLine {
    buf: Box<[f32]>,
    scratch: Box<[f32]>,
    pos: usize,
}

impl DelayLine {
    fn new(delay_frames: usize, channels: usize) -> Self {
        Self {
            buf: vec![0.0; delay_frames * channels].into_boxed_slice(),
            scratch: vec![0.0; DSP_BLOCK_FRAMES * channels].into_boxed_slice(),
            pos: 0,
        }
    }

    fn delayed<'a>(&'a mut self, input: &'a [f32]) -> &'a [f32] {
        let cap = self.buf.len();
        if cap == 0 {
            return input;
        }
        let n = input.len().min(self.scratch.len());
        let mut pos = self.pos;
        for i in 0..n {
            self.scratch[i] = self.buf[pos];
            self.buf[pos] = input[i];
            pos = if pos + 1 == cap { 0 } else { pos + 1 };
        }
        self.pos = pos;
        &self.scratch[..n]
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
    /// Blocks produced by `process_block`. A clone lives in this build's
    /// `BuiltOutputGraph::output` so the non-RT tick thread can compare this
    /// worker's real block rate against `sample_rate / DSP_BLOCK_FRAMES`.
    blocks: Arc<AtomicU64>,
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

    /// Attach a publish ring to a fan-out effect node; its `out_buf` is pushed
    /// there each block for another output's ring-source to read.
    pub(super) fn attach_tap(&mut self, node_idx: usize, prod: Producer<f32>) {
        if let Some(DagNode::Effect(e)) = self.nodes.get_mut(node_idx) {
            e.taps.push(prod);
        }
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
        self.blocks.fetch_add(1, Ordering::Relaxed);
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
                    let target = edge.target_handle.as_deref();
                    let src = match &mut edge.delay {
                        Some(d) => d.delayed(src),
                        None => src,
                    };
                    let Some((_, buf)) = cons
                        .channel_bufs
                        .iter_mut()
                        .find(|(h, _)| Some(h.as_str()) == target)
                    else {
                        continue;
                    };
                    add_mapped(src, buf);
                }
                for (i, (_, buf)) in cons.channel_bufs.iter().enumerate() {
                    if let Some(prod) = cons.send_producers.get_mut(i) {
                        bulk_push_counted(prod, buf, &health::TAP_RING_OVERRUN_SAMPLES);
                    }
                }
                continue;
            }
            if let DagNode::Effect(eff) = &mut tail[0] {
                for s in eff.out_buf.iter_mut() {
                    *s = 0.0;
                }
                for edge in &mut eff.incoming {
                    let src = head[edge.src_idx].out_buf_for_handle(edge.source_handle.as_deref());
                    let route = edge.target_handle.as_deref().and_then(target_route);
                    let src = match &mut edge.delay {
                        Some(d) => d.delayed(src),
                        None => src,
                    };
                    match route {
                        Some((off, 1)) => add_to_channel(src, &mut eff.out_buf, off),
                        Some((off, _)) => add_block_at(src, &mut eff.out_buf, off),
                        None => add_mapped(src, &mut eff.out_buf),
                    }
                }
                if let Some(sc_buf) = eff.sidechain_buf.as_mut() {
                    for s in sc_buf.iter_mut() {
                        *s = 0.0;
                    }
                    for edge in &mut eff.sidechain {
                        let src =
                            head[edge.src_idx].out_buf_for_handle(edge.source_handle.as_deref());
                        let src = match &mut edge.delay {
                            Some(d) => d.delayed(src),
                            None => src,
                        };
                        add_mapped(src, sc_buf);
                    }
                }
                if !eff.bypass.load(Ordering::Relaxed) {
                    eff.run(DSP_BLOCK_FRAMES);
                }
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
                // Publish the processed block to every consuming output's ring.
                for prod in eff.taps.iter_mut() {
                    bulk_push_counted(prod, &eff.out_buf, &health::TAP_RING_OVERRUN_SAMPLES);
                }
            }
        }
        for s in output.iter_mut() {
            *s = 0.0;
        }
        for terminal in &mut self.terminals {
            let src =
                self.nodes[terminal.src_idx].out_buf_for_handle(terminal.source_handle.as_deref());
            let src = match &mut terminal.delay {
                Some(d) => d.delayed(src),
                None => src,
            };
            match terminal.route {
                Some((off, 1)) => add_to_channel(src, output, off),
                Some((off, _)) => add_block_at(src, output, off),
                None => add_mapped(src, output),
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
    pub sources: Vec<SourceMeta>,
    pub output: OutputMeta,
    /// Effect node id -> (node index, channel width). Used to attach publish
    /// taps to nodes that fan out to other outputs.
    pub node_meta: HashMap<String, (usize, usize)>,
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
    // Effect nodes provided by a ring instead of built here: each is computed
    // once in its owning output's graph and read back as a ring-source. Maps
    // node id -> (ring consumer, owner-graph sample rate, channel width).
    mut cut_leaves: HashMap<String, (Consumer<f32>, u32, usize)>,
) -> AppResult<BuiltOutputGraph> {
    let cut_leaf_ids: HashSet<String> = cut_leaves.keys().cloned().collect();
    let reachable: HashSet<String> = match output_id {
        Some(id) => reachable_backward_cut(id, valid, &cut_leaf_ids),
        // Monitor: everything feeding an analyzer, stopping at cut nodes (whose
        // processed output is read back from the owning output's ring).
        None => {
            let roots: Vec<String> = valid
                .effects
                .iter()
                .filter(|e| is_analyzer(&e.spec))
                .map(|e| e.id.clone())
                .collect();
            reachable_backward_from(&roots, valid, &cut_leaf_ids)
        }
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
    // Effect node id -> (index in `nodes`, channel width). Lets the caller wire
    // publish taps onto a node that fans out to other outputs' ring-sources.
    let mut node_meta: HashMap<String, (usize, usize)> = HashMap::new();
    let mut controls: Vec<(String, EffectControl)> = Vec::new();
    let mut bypasses: Vec<(String, Arc<AtomicBool>)> = Vec::new();
    let mut meters: Vec<MeterHandle> = Vec::new();
    let mut lufs: Vec<LufsHandle> = Vec::new();
    let mut gr_handles: Vec<GrHandle> = Vec::new();
    let mut scopes: Vec<WaveformHandle> = Vec::new();
    let mut sources: Vec<SourceMeta> = Vec::new();
    let mut node_latencies: Vec<usize> = Vec::with_capacity(topo.len());
    // Per-node channel width; effects inherit the max width of their upstreams.
    let mut node_channels: Vec<usize> = Vec::with_capacity(topo.len());

    for id in &topo {
        // A fan-out node owned by an earlier output: read its published block
        // from the ring instead of rebuilding the whole upstream chain.
        if let Some((consumer, owner_sr, width)) = cut_leaves.remove(id) {
            let source = ring_source(id, consumer, owner_sr, output_sr, width, realtime, valid)?;
            sources.push(SourceMeta {
                label: format!("{} out={}", source.label, output_id.unwrap_or("monitor")),
                stats: source.stats.clone(),
                channels: width,
                native_sr: owner_sr,
                frames_per_block: source.input_samples_per_block / width.max(1),
                input_id: None,
                output_id: output_id.unwrap_or("monitor").to_string(),
                capture: None,
            });
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Source(source));
            node_latencies.push(0);
            node_channels.push(width);
            continue;
        }
        if let Some(input) = valid.inputs.iter().find(|i| &i.id == id) {
            // Network producers are not captured sources: they emit per-channel
            // outputs from a shared jitter buffer at the output rate. The handle
            // naming is all that separates the two -- direct-IP draws `chN` off
            // one sender, WebRTC draws `peer:<id>[:<ch>]` off many.
            let network = match &input.spec {
                InputSpec::NetReceiver { port } => {
                    let receiver = crate::audio::netaudio::receiver::get_or_create(id, *port);
                    Some(ChannelReceiver::new(
                        receiver.register_consumer(output_sr, realtime),
                    ))
                }
                InputSpec::WebRtcRecv { node_id, opus_bitrate, opus_application } => {
                    let session = crate::audio::webrtc::get_or_create(
                        node_id,
                        *opus_bitrate,
                        *opus_application,
                    );
                    Some(ChannelReceiver::new(
                        session.register_bridge(output_sr, realtime),
                    ))
                }
                _ => None,
            };
            if let Some(receiver) = network {
                let mut handles: Vec<String> = valid
                    .edges
                    .iter()
                    .filter(|e| &e.from == id)
                    .filter_map(|e| e.source_handle.clone())
                    .collect();
                handles.sort();
                handles.dedup();
                let pw = 2;
                let mut handle_bufs = Vec::with_capacity(handles.len());
                let mut wire_keys = Vec::with_capacity(handles.len());
                for h in handles {
                    let Some(key) = tap_key(&h) else { continue };
                    wire_keys.push(key);
                    handle_bufs.push((h, vec![0.0; DSP_BLOCK_FRAMES]));
                }
                id_to_index.insert(id.clone(), nodes.len());
                nodes.push(DagNode::Producer(ProducerState {
                    receiver,
                    out_buf: vec![0.0; DSP_BLOCK_FRAMES * pw],
                    handle_bufs,
                    wire_keys,
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
            let source_channels = input_native_channels.get(id).copied().unwrap_or(2) as usize;
            // Scale by channels to keep the buffered span constant in time; at
            // high channel counts a smaller cushion starves on capture-clock drift.
            let (producer, consumer) =
                RingBuffer::<f32>::new(RING_CAPACITY_FRAMES * source_channels);
            producer_pairs.push((id.clone(), producer));
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

            let kind = match &input.spec {
                InputSpec::Microphone { device_id } => format!("mic:{device_id}"),
                InputSpec::SystemAudio { .. } => "system-audio".to_string(),
                InputSpec::AppAudio { bundle_id } => format!("app:{bundle_id}"),
                InputSpec::AudioFile { file_path } => format!("file:{file_path}"),
                InputSpec::NetReceiver { .. } | InputSpec::WebRtcRecv { .. } => {
                    unreachable!("network inputs are built as producers")
                }
            };
            let label = format!("{kind}@{input_sr}->{output_sr} out={}", output_id.unwrap_or("monitor"));
            let stats = SourceStats::new();
            sources.push(SourceMeta {
                label: label.clone(),
                stats: stats.clone(),
                channels: source_channels,
                native_sr: input_sr,
                frames_per_block: input_frames_per_block as usize,
                input_id: Some(id.clone()),
                output_id: output_id.unwrap_or("monitor").to_string(),
                capture: None,
            });
            let source = SourceState {
                label,
                channels: source_channels,
                consumer,
                resampler,
                input_staging: Vec::with_capacity(
                    (RESAMPLE_CHUNK + SPLICE_FADE_FRAMES) * source_channels + 8,
                ),
                splice_tmp: Vec::with_capacity(SPLICE_FADE_FRAMES * source_channels),
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
                stats,
            };
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Source(source));
            node_latencies.push(0);
            node_channels.push(source_channels);
        } else if let Some(effect) = valid.effects.iter().find(|e| &e.id == id) {
            // The cut plan builds each node in exactly one graph (its owner --
            // a real output, or the monitor for analyzer-only nodes), so this
            // build is the sole plugin instance and always the editor target.
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
                .map(|(i, sh, _)| edge_channels(&nodes, &node_channels, *i, sh.as_deref()))
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
            let eff_channels = upstream_w.max(target_w).max(tap_w).max(1);
            // Built once the node's width is known: a plugin is offered that
            // width and may take it whole, the way a DAW instantiates one
            // multichannel plugin instead of several stereo ones.
            let build = instantiate_effect(
                &effect.spec, id, output_sr, realtime, true, eff_channels, registry,
            );
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
            let make_edge = |src_idx: usize,
                             source_handle: Option<String>,
                             target_handle: Option<String>| {
                let pad = max_upstream - node_latencies[src_idx];
                let width = edge_channels(&nodes, &node_channels, src_idx, source_handle.as_deref());
                IncomingEdge {
                    src_idx,
                    source_handle,
                    target_handle,
                    delay: if pad > 0 {
                        Some(DelayLine::new(pad, width))
                    } else {
                        None
                    },
                }
            };
            let incoming: Vec<IncomingEdge> = main_upstream
                .into_iter()
                .map(|(i, s, t)| make_edge(i, s, t))
                .collect();
            let sidechain: Vec<IncomingEdge> = side_upstream
                .into_iter()
                .map(|(i, s, t)| make_edge(i, s, t))
                .collect();
            let sidechain_buf = if sidechain.is_empty() {
                None
            } else {
                Some(vec![0.0; DSP_BLOCK_FRAMES * eff_channels])
            };
            // Generic `chK` per-channel taps drawn off this effect. A stale
            // handle just yields silence.
            let mut handle_ids: Vec<String> = valid
                .edges
                .iter()
                .filter(|e| &e.from == id)
                .filter_map(|e| e.source_handle.clone())
                .filter(|h| tap_handle_width(h).is_some())
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
            // Analyzers read all channels at once, and so does a plugin that
            // accepted the node's full width. Everything else runs one instance
            // per stereo pair.
            let full_width = build.full_width
                || matches!(
                    effect.spec,
                    EffectSpec::LevelMeter(_) | EffectSpec::Waveform(_) | EffectSpec::Spectrum(_)
                );
            let pairs = if full_width {
                1
            } else {
                eff_channels.div_ceil(2)
            };
            let mut effects = Vec::with_capacity(pairs);
            let own = build.effect.latency_frames();
            effects.push(build.effect);
            for _ in 1..pairs {
                // Extra stereo pairs are separate instances for wider audio,
                // never the editor target.
                // Extra pairs exist only when the node is driven pairwise, so
                // each is asked for stereo rather than the node's full width.
                let extra =
                    instantiate_effect(&effect.spec, id, output_sr, realtime, false, 2, registry);
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
                taps: Vec::new(),
            }));
            node_meta.insert(id.clone(), (nodes.len() - 1, eff_channels));
            node_latencies.push(max_upstream + own);
            node_channels.push(eff_channels);
        }
    }

    // Matches the source label style (`out=<id>` / "monitor").
    let out_label = output_id.map(|id| format!("out={id}")).unwrap_or_else(|| "monitor".to_string());
    let blocks = Arc::new(AtomicU64::new(0));

    // A wire sender (direct-IP or WebRTC) is a terminal Consumer node inside the
    // DAG (not a summed output terminal): it sums per-channel inputs and pushes
    // them into send rings drained by a background transmitter.
    let wire_sender = output_id
        .and_then(|oid| valid.outputs.iter().find(|o| o.id == oid))
        .and_then(|o| match &o.spec {
            OutputSpec::NetSender { .. } | OutputSpec::WebRtcSend { .. } => Some(o.spec.clone()),
            _ => None,
        });
    if let Some(spec) = wire_sender {
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
                let width = edge_channels(&nodes, &node_channels, idx, source_handle.as_deref());
                IncomingEdge {
                    src_idx: idx,
                    source_handle,
                    target_handle,
                    delay: if pad > 0 {
                        Some(DelayLine::new(pad, width))
                    } else {
                        None
                    },
                }
            })
            .collect();

        let channels = match &spec {
            OutputSpec::NetSender { channels, .. } | OutputSpec::WebRtcSend { channels, .. } => {
                *channels
            }
            _ => unreachable!("wire sender spec"),
        };
        let n = channels.clamp(1, MAX_NET_CH) as usize;
        let mut channel_bufs: Vec<(String, Vec<f32>)> = Vec::with_capacity(n);
        let mut send_producers: Vec<Producer<f32>> = Vec::with_capacity(n);
        let mut send_consumers: Vec<Consumer<f32>> = Vec::with_capacity(n);
        for c in 1..=n {
            channel_bufs.push((format!("ch{c}"), vec![0.0; DSP_BLOCK_FRAMES]));
            let (prod, cons) = RingBuffer::<f32>::new(crate::audio::netaudio::SEND_RING);
            send_producers.push(prod);
            send_consumers.push(cons);
        }
        match &spec {
            OutputSpec::NetSender {
                node_id,
                target,
                codec,
                opus_bitrate,
                opus_application,
                ..
            } => {
                let format = match codec {
                    NetCodec::PcmF32 => Format::PcmF32,
                    NetCodec::PcmI16 => Format::PcmI16,
                    NetCodec::Opus => Format::Opus,
                };
                let sender = crate::audio::netaudio::sender::get_or_create(
                    node_id,
                    *target,
                    format,
                    *opus_bitrate,
                    *opus_application,
                );
                sender.set_send_consumers(send_consumers);
            }
            OutputSpec::WebRtcSend { node_id, opus_bitrate, opus_application, .. } => {
                let session = crate::audio::webrtc::get_or_create(
                    node_id,
                    *opus_bitrate,
                    *opus_application,
                );
                // This graph already runs at the wire rate, so the encode task's
                // own resampler stays out of the path.
                session.set_send_consumers(send_consumers, output_sr);
            }
            _ => unreachable!("wire sender spec"),
        }

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
                blocks: blocks.clone(),
            },
            controls,
            bypasses,
            meters,
            lufs,
            gr_handles,
            scopes,
            sources,
            output: OutputMeta { label: out_label, blocks, sample_rate: output_sr, channels: 2, io: None },
            node_meta,
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
                    let width =
                        edge_channels(&nodes, &node_channels, src_idx, source_handle.as_deref());
                    TerminalEdge {
                        src_idx,
                        source_handle,
                        route,
                        delay: if pad > 0 {
                            Some(DelayLine::new(pad, width))
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
            blocks: blocks.clone(),
        },
        controls,
        bypasses,
        meters,
        lufs,
        gr_handles,
        scopes,
        sources,
        output: OutputMeta { label: out_label, blocks, sample_rate: output_sr, channels: 2, io: None },
        node_meta,
    })
}

/// Builds a `SourceState` that reads a fan-out node's published block from a
/// ring (written at `owner_sr`) and resamples it to this graph's `output_sr`.
/// Reuses the source machinery so per-channel taps and backlog-dropping behave
/// exactly like a captured input.
#[allow(clippy::too_many_arguments)]
fn ring_source(
    id: &str,
    consumer: Consumer<f32>,
    owner_sr: u32,
    output_sr: u32,
    channels: usize,
    realtime: bool,
    valid: &ValidGraph,
) -> AppResult<SourceState> {
    let resampler = if owner_sr == output_sr {
        None
    } else {
        Some(MultiResampler::new(owner_sr, output_sr, RESAMPLE_CHUNK, channels)?)
    };
    let input_frames_per_block =
        (DSP_BLOCK_FRAMES as u64 * owner_sr as u64 + output_sr as u64 - 1) / output_sr as u64;
    let input_samples_per_block = input_frames_per_block as usize * channels;

    let mut ch_handles: Vec<String> = valid
        .edges
        .iter()
        .filter(|e| e.from == id)
        .filter_map(|e| e.source_handle.clone())
        .filter(|h| tap_handle_width(h).is_some())
        .collect();
    ch_handles.sort();
    ch_handles.dedup();
    let handle_bufs: Vec<(String, Vec<f32>)> = ch_handles
        .into_iter()
        .map(|h| {
            let w = tap_handle_width(&h).unwrap_or(1);
            (h, vec![0.0; DSP_BLOCK_FRAMES * w])
        })
        .collect();

    Ok(SourceState {
        label: format!("cut:{id}"),
        channels,
        consumer,
        resampler,
        input_staging: Vec::with_capacity((RESAMPLE_CHUNK + SPLICE_FADE_FRAMES) * channels + 8),
        splice_tmp: Vec::with_capacity(SPLICE_FADE_FRAMES * channels),
        out_pending: StagingRing::with_capacity(RESAMPLE_CHUNK * channels * 4 + DSP_BLOCK_FRAMES * channels),
        chunk_tmp: Vec::with_capacity(RESAMPLE_CHUNK * channels + 8),
        out_buf: vec![0.0; DSP_BLOCK_FRAMES * channels],
        input_samples_per_block,
        realtime,
        last_pop_at: Instant::now(),
        first_data_logged: false,
        volume: Arc::new(AtomicU32::new(0x3F80_0000)),
        paused: None,
        drain: None,
        last_drain_gen: 0,
        meter: None,
        handle_bufs,
        stats: SourceStats::new(),
    })
}

/// Cross-output fan-out plan: which effect nodes are computed once and shared
/// via rings. `owner[n]` builds node `n` and publishes it; every output in
/// `consumers[n]` reads it back as a ring-source.
pub(super) struct CutPlan {
    pub owner: HashMap<String, String>,
    pub consumers: HashMap<String, Vec<String>>,
}

impl CutPlan {
    /// Outputs that participate in any cut (owners + consumers). When one of
    /// them is rebuilt they must all rebuild together, so producer and consumer
    /// ends of every ring are created in the same pass.
    pub(super) fn participants(&self) -> HashSet<String> {
        let mut set = HashSet::new();
        for (node, cons) in &self.consumers {
            if cons.is_empty() {
                continue;
            }
            if let Some(o) = self.owner.get(node) {
                set.insert(o.clone());
            }
            set.extend(cons.iter().cloned());
        }
        set
    }
}

/// Assigns each effect node to the first output (in graph order) that can
/// compute it, and records where later graphs must read it back via a ring.
/// Traversal stops at nodes already owned by an earlier graph -- those become
/// ring-source leaves -- so a shared node is computed exactly once. The monitor
/// (identified by `monitor_key`) is treated as a final consumer, so a plugin
/// feeding both a speaker and an analyzer is computed once, not duplicated.
pub(super) fn plan_cuts(valid: &ValidGraph, monitor_key: Option<&str>) -> CutPlan {
    let effect_ids: HashSet<&str> = valid.effects.iter().map(|e| e.id.as_str()).collect();
    let mut owner: HashMap<String, String> = HashMap::new();
    let mut consumers: HashMap<String, Vec<String>> = HashMap::new();

    let mut assign = |oid: &str, starts: Vec<String>| {
        let mut visited: HashSet<String> = HashSet::new();
        let mut stack = starts;
        while let Some(m) = stack.pop() {
            // Only effect nodes are cut; inputs already fan out via their own
            // per-output source rings.
            if !effect_ids.contains(m.as_str()) {
                continue;
            }
            if owner.contains_key(&m) {
                consumers.entry(m).or_default().push(oid.to_string());
                continue;
            }
            if !visited.insert(m.clone()) {
                continue;
            }
            for e in &valid.edges {
                if e.to == m {
                    stack.push(e.from.clone());
                }
            }
        }
        for m in visited {
            owner.insert(m, oid.to_string());
        }
    };

    for out in &valid.outputs {
        let starts = valid
            .edges
            .iter()
            .filter(|e| e.to == out.id)
            .map(|e| e.from.clone())
            .collect();
        assign(&out.id, starts);
    }
    // Monitor last: it reaches every analyzer, so shared nodes owned by a real
    // output are read from their ring and only monitor-only nodes stay local.
    if let Some(mk) = monitor_key {
        let starts = valid
            .effects
            .iter()
            .filter(|e| is_analyzer(&e.spec))
            .map(|e| e.id.clone())
            .collect();
        assign(mk, starts);
    }

    for v in consumers.values_mut() {
        v.dedup();
    }
    CutPlan { owner, consumers }
}

/// Analyzer effects are monitor-graph roots: they render telemetry and have no
/// audio successor, so the monitor sub-graph is everything that feeds one.
fn is_analyzer(spec: &EffectSpec) -> bool {
    matches!(
        spec,
        EffectSpec::LevelMeter(_)
            | EffectSpec::LufsMeter(_)
            | EffectSpec::Waveform(_)
            | EffectSpec::Spectrum(_)
    )
}

/// Like `reachable_backward` but does not expand through `stop` nodes: they are
/// included as leaves (built as ring-sources) but their upstream chain is not.
fn reachable_backward_cut(
    output_id: &str,
    valid: &ValidGraph,
    stop: &HashSet<String>,
) -> HashSet<String> {
    let starts: Vec<String> = valid
        .edges
        .iter()
        .filter(|e| e.to == output_id)
        .map(|e| e.from.clone())
        .collect();
    reachable_backward_from(&starts, valid, stop)
}

/// Backward reachability from a set of start nodes (the starts are included),
/// not expanding through `stop` nodes.
fn reachable_backward_from(
    starts: &[String],
    valid: &ValidGraph,
    stop: &HashSet<String>,
) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack: Vec<String> = starts.to_vec();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if stop.contains(&id) {
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
    use super::{add_mapped, crossfade_into, DelayLine, DSP_BLOCK_FRAMES, SPLICE_FADE_FRAMES};

    // Latency compensation on a branch that bypasses a latent effect must be a
    // pure delay: same samples, same order, only shifted.
    #[test]
    fn delay_line_shifts_without_losing_samples() {
        const PAD_FRAMES: usize = 482;
        let mut line = DelayLine::new(PAD_FRAMES, 2);
        let mut fed: Vec<f32> = Vec::new();
        let mut got: Vec<f32> = Vec::new();
        for b in 0..4 {
            let mut input = vec![0.0_f32; DSP_BLOCK_FRAMES * 2];
            for f in 0..DSP_BLOCK_FRAMES {
                let v = (b * DSP_BLOCK_FRAMES + f) as f32;
                input[f * 2] = v;
                input[f * 2 + 1] = -v;
            }
            fed.extend_from_slice(&input);
            let mut dst = vec![0.0_f32; DSP_BLOCK_FRAMES * 2];
            add_mapped(line.delayed(&input), &mut dst);
            got.extend_from_slice(&dst);
        }
        let shift = PAD_FRAMES * 2;
        for i in shift..got.len() {
            assert_eq!(got[i], fed[i - shift], "sample {i} differs");
        }
    }

    // A mono tap drawn off a stereo node carries one channel, not two. Sizing
    // the line by the node's width instead of the edge's dropped half of every
    // block and paired consecutive samples as L/R, doubling the pitch.
    #[test]
    fn delay_line_fills_a_whole_mono_block() {
        const PAD_FRAMES: usize = 482;
        let mut line = DelayLine::new(PAD_FRAMES, 1);
        let mut fed: Vec<f32> = Vec::new();
        let mut got: Vec<f32> = Vec::new();
        for b in 0..4 {
            let mut input = vec![0.0_f32; DSP_BLOCK_FRAMES];
            for (f, s) in input.iter_mut().enumerate() {
                *s = (b * DSP_BLOCK_FRAMES + f) as f32 + 1.0;
            }
            fed.extend_from_slice(&input);
            let mut dst = vec![0.0_f32; DSP_BLOCK_FRAMES * 2];
            add_mapped(line.delayed(&input), &mut dst);
            got.extend_from_slice(&dst);
        }
        // Mono upmixes to both channels, and no frame of any block stays silent.
        for b in 1..4 {
            for f in 0..DSP_BLOCK_FRAMES {
                let i = b * DSP_BLOCK_FRAMES * 2 + f * 2;
                assert_ne!(got[i], 0.0, "left silent at block {b} frame {f}");
                assert_eq!(got[i], got[i + 1], "channels differ at block {b} frame {f}");
            }
        }
        for f in PAD_FRAMES..fed.len() {
            assert_eq!(got[f * 2], fed[f - PAD_FRAMES], "frame {f} differs");
        }
    }

    // A trim's join must be continuous at both ends, or the splice it was meant
    // to hide becomes two smaller steps.
    #[test]
    fn crossfade_joins_without_a_step() {
        const CH: usize = 2;
        let mut dst = vec![1.0_f32; SPLICE_FADE_FRAMES * CH];
        let incoming = vec![0.0_f32; SPLICE_FADE_FRAMES * CH];
        crossfade_into(&mut dst, &incoming, &[], CH);

        assert_eq!(dst[0], 1.0, "first frame must stay pure outgoing");
        assert_eq!(dst[1], 1.0, "both channels of the first frame agree");
        let last = (SPLICE_FADE_FRAMES - 1) * CH;
        assert_eq!(dst[last], 0.0, "last frame must reach pure incoming");
        assert_eq!(dst[last + 1], 0.0, "both channels of the last frame agree");

        for f in 1..SPLICE_FADE_FRAMES {
            assert!(dst[f * CH] < dst[(f - 1) * CH], "fade must be monotonic");
            assert_eq!(dst[f * CH], dst[f * CH + 1], "channels share a weight");
        }
    }

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
