use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer};
use tracing::{info, warn};

use crate::audio::effects::{update_meter, MeterHandle, RuntimeEffect};
use crate::audio::health;
use crate::audio::input_bridge::CaptureStats;
use crate::audio::resample::MultiResampler;
use crate::audio::stream_recv::ChannelReceiver;

use super::graph::DelayLine;
use super::staging::StagingRing;
use super::{DSP_BLOCK_FRAMES, RESAMPLE_CHUNK};

/// How long a source can go without delivering before the availability-paced
/// worker stops waiting on it. SCK in normal operation delivers every ~20 ms,
/// so 150 ms is ~7x headroom -- enough to avoid false positives on bursty
/// delivery, short enough that a real stall doesn't drown the FAST source's
/// ring buffer.
pub(super) const STALL_THRESHOLD: Duration = Duration::from_millis(150);

const SOURCE_BACKLOG_HIGH_BLOCKS: usize = 4;
const SOURCE_BACKLOG_LOW_BLOCKS: usize = 2;
/// Ceiling on one block's trim. Backlog drains over a few seconds instead of
/// vanishing in a single splice, which is what makes it inaudible.
const TRIM_MAX_FRAMES_PER_BLOCK: usize = 64;
/// Crossfade length across a trim's cut. Long enough to kill the step, short
/// enough that the replayed audio reads as texture rather than an echo.
pub(super) const SPLICE_FADE_FRAMES: usize = 32;

/// One node in an output's DAG. `Source` reads from a ring + resamples,
/// `Effect` sums its upstreams' buffers and runs DSP, `Producer` emits
/// network-received audio on named channel handles. Each exposes an
/// interleaved `out_buf` of `DSP_BLOCK_FRAMES * node_channels` that downstream
/// nodes consume.
pub(super) enum DagNode {
    Source(SourceState),
    Effect(EffectState),
    Producer(ProducerState),
    Consumer(ConsumerState),
}

impl DagNode {
    pub(super) fn out_buf(&self) -> &[f32] {
        match self {
            DagNode::Source(s) => &s.out_buf,
            DagNode::Effect(e) => &e.out_buf,
            DagNode::Producer(p) => &p.out_buf,
            // Terminal sink; validation forbids outgoing edges, so never read.
            DagNode::Consumer(_) => &[],
        }
    }

    /// Unknown or absent handles fall back to the node's main `out_buf`.
    pub(super) fn out_buf_for_handle(&self, handle: Option<&str>) -> &[f32] {
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
pub struct SourceStats {
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
    pub(super) fn new() -> Self {
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
pub struct SourceMeta {
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
pub(in crate::audio::pipeline) struct OutputMeta {
    pub label: String,
    pub blocks: Arc<AtomicU64>,
    pub sample_rate: u32,
    pub channels: usize,
    pub io: Option<crate::audio::pipeline::output::SpeakerIo>,
}

pub(super) struct SourceState {
    pub(super) label: String,
    pub(super) channels: usize,
    pub(super) consumer: Consumer<f32>,
    pub(super) resampler: Option<MultiResampler>,
    pub(super) input_staging: Vec<f32>,
    /// Holds a trim's crossfaded join until the refill path picks it up.
    pub(super) splice_tmp: Vec<f32>,
    pub(super) out_pending: StagingRing,
    pub(super) chunk_tmp: Vec<f32>,
    pub(super) out_buf: Vec<f32>,
    pub(super) input_samples_per_block: usize,
    pub(super) realtime: bool,
    /// >STALL_THRESHOLD since last pop => zero-fill and stop waiting on this source.
    pub(super) last_pop_at: Instant,
    pub(super) first_data_logged: bool,
    pub(super) volume: Arc<AtomicU32>,
    pub(super) paused: Option<Arc<AtomicBool>>,
    // u64 generation (not AtomicBool) so every output's SourceState detects the
    // seek independently; swap(false) would clear the flag for the first reader.
    pub(super) drain: Option<Arc<AtomicU64>>,
    pub(super) last_drain_gen: u64,
    pub(super) meter: Option<MeterHandle>,
    // Per-channel taps ("chK") drawn off this source.
    pub(super) handle_bufs: Vec<(String, Vec<f32>)>,
    pub(super) stats: SourceStats,
}

impl SourceState {
    pub(super) fn is_stalled(&self) -> bool {
        self.last_pop_at.elapsed() > STALL_THRESHOLD
    }

    /// Taps are filled at the end of `fill_block`, so an early return would
    /// leave them looping their last block -- a buzz at the block rate.
    pub(super) fn silence(&mut self) {
        self.out_buf.fill(0.0);
        for (_, buf) in self.handle_bufs.iter_mut() {
            buf.fill(0.0);
        }
    }

    pub(super) fn fill_block(&mut self) {
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
                    self.stats
                        .consumed
                        .fetch_add(avail as u64, Ordering::Relaxed);
                    self.last_pop_at = Instant::now();
                }
            }
            if self.input_staging.len() < needed {
                return;
            }
            self.chunk_tmp.clear();
            if let Err(e) = rs.process_chunk(&self.input_staging[..needed], &mut self.chunk_tmp) {
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
                    self.stats
                        .consumed
                        .fetch_add(avail as u64, Ordering::Relaxed);
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

pub(super) struct EffectState {
    pub(super) effects: Vec<RuntimeEffect>,
    pub(super) full_width: bool,
    pub(super) bypass: Arc<AtomicBool>,
    pub(super) incoming: Vec<IncomingEdge>,
    pub(super) sidechain: Vec<IncomingEdge>,
    pub(super) out_buf: Vec<f32>,
    pub(super) sidechain_buf: Option<Vec<f32>>,
    pub(super) pair_main: Vec<f32>,
    pub(super) pair_side: Vec<f32>,
    pub(super) handle_bufs: Vec<(String, Vec<f32>)>,
    pub(super) taps: Vec<Producer<f32>>,
}

impl EffectState {
    pub(super) fn run(&mut self, frames: usize) {
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

pub(super) struct ProducerState {
    pub(super) receiver: ChannelReceiver,
    pub(super) out_buf: Vec<f32>,
    pub(super) handle_bufs: Vec<(String, Vec<f32>)>,
    pub(super) wire_keys: Vec<TapKey>,
}

pub(super) enum TapKey {
    Channel(String),
    PrefixMix(String),
}

impl ProducerState {
    pub(super) fn process(&mut self) {
        self.receiver.mix_block(&mut self.out_buf);
        for ((_, buf), key) in self.handle_bufs.iter_mut().zip(&self.wire_keys) {
            match key {
                TapKey::Channel(k) => self.receiver.channel(k, buf),
                TapKey::PrefixMix(p) => self.receiver.prefix_mix(p, buf),
            }
        }
    }
}

pub(super) struct ConsumerState {
    pub(super) incoming: Vec<IncomingEdge>,
    pub(super) channel_bufs: Vec<(String, Vec<f32>)>,
    pub(super) send_producers: Vec<Producer<f32>>,
}

pub(super) struct IncomingEdge {
    pub(super) src_idx: usize,
    pub(super) source_handle: Option<String>,
    pub(super) target_handle: Option<String>,
    pub(super) delay: Option<DelayLine>,
}

pub(super) struct TerminalEdge {
    pub(super) src_idx: usize,
    pub(super) source_handle: Option<String>,
    pub(super) route: Option<(usize, usize)>,
    pub(super) delay: Option<DelayLine>,
}

#[inline]
pub(super) fn parse_ch(handle: &str) -> Option<usize> {
    handle
        .strip_prefix("ch")
        .and_then(|s| s.parse::<usize>().ok())
}

pub(super) fn tap_key(handle: &str) -> Option<TapKey> {
    if let Some(rest) = handle.strip_prefix("peer:") {
        return Some(if rest.contains(':') {
            TapKey::Channel(rest.to_string())
        } else {
            TapKey::PrefixMix(format!("{rest}:"))
        });
    }
    parse_ch(handle).map(|ch| TapKey::Channel((ch - 1).to_string()))
}

#[inline]
pub(super) fn parse_stereo(handle: &str) -> Option<usize> {
    handle
        .strip_prefix("st")
        .and_then(|s| s.parse::<usize>().ok())
}

pub(super) fn edge_channels(
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

#[inline]
pub(super) fn tap_handle_width(handle: &str) -> Option<usize> {
    if parse_stereo(handle).is_some() {
        Some(2)
    } else if parse_ch(handle).is_some() {
        Some(1)
    } else {
        None
    }
}

#[inline]
pub(super) fn target_route(handle: &str) -> Option<(usize, usize)> {
    if let Some(a) = parse_stereo(handle) {
        Some((a - 1, 2))
    } else if let Some(k) = parse_ch(handle) {
        Some((k - 1, 1))
    } else {
        None
    }
}

#[inline]
pub(super) fn add_mapped(src: &[f32], dst: &mut [f32]) {
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

#[inline]
pub(super) fn add_to_channel(src: &[f32], dst: &mut [f32], ch: usize) {
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

#[inline]
pub(super) fn add_block_at(src: &[f32], dst: &mut [f32], off: usize) {
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

pub(super) fn crossfade_into(dst: &mut [f32], first: &[f32], second: &[f32], channels: usize) {
    let span = (SPLICE_FADE_FRAMES - 1).max(1) as f32;
    for (i, s) in first.iter().chain(second.iter()).enumerate() {
        if i >= dst.len() {
            break;
        }
        let w = ((i / channels) as f32 / span).min(1.0);
        dst[i] = dst[i] * (1.0 - w) + s * w;
    }
}
