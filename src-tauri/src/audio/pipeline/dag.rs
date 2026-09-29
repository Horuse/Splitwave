use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};

use super::asrc::Asrc;
use super::cushion::{Adjust, Cushion, MAX_SPLICE_FRAMES};
use super::latency::NodeTiming;
use super::sig::MONITOR_KEY;
use crate::audio::effects::{
    instantiate_effect, update_meter, EffectControl, EffectRegistry, GrHandle, LufsHandle,
    MeterHandle, RuntimeEffect, WaveformHandle,
};
use crate::audio::graph::{EdgeKind, EffectSpec, InputSpec, NetCodec, OutputSpec, ValidGraph};
use crate::audio::health;
use crate::audio::input_bridge::{now_secs, CaptureStats, WriteClock};
use crate::audio::netaudio::packet::Format;
use crate::audio::resample::MultiResampler;
use crate::audio::stream_recv::ChannelReceiver;
use crate::audio::streams::bulk_push_counted;
use crate::error::{AppError, AppResult};

/// One second of frames at the ring's own clock rate.
pub(super) fn ring_capacity_frames(sample_rate: u32) -> usize {
    sample_rate.max(1) as usize
}

/// Block size used by the resampler. 256 frames @ 48 kHz ~ 5.3 ms.
pub(super) const RESAMPLE_CHUNK: usize = 256;

/// Block of the timer-paced workers (recording, monitoring, wire senders).
/// Nobody hears their latency, and a timer cannot pace small blocks reliably.
/// Speaker graphs run at the engine buffer size instead.
pub const TIMER_BLOCK_FRAMES: usize = 1024;

const MAX_NET_CH: u32 = crate::audio::netaudio::MAX_CHANNELS as u32;

/// How long a source can go without delivering before the availability-paced
/// worker stops waiting on it. SCK in normal operation delivers every ~20 ms,
/// so 150 ms is ~7x headroom -- enough to avoid false positives on bursty
/// delivery, short enough that a real stall doesn't drown the FAST source's
/// ring buffer.
const STALL_THRESHOLD: Duration = Duration::from_millis(150);

/// Crossfade length across a trim's cut. Long enough to kill the step, short
/// enough that the replayed audio reads as texture rather than an echo.
pub(super) const SPLICE_FADE_FRAMES: usize = 32;

/// Fades `len` frames of interleaved `buf` starting at frame `from`, in
/// (`up`) or out, over at most `SPLICE_FADE_FRAMES`.
fn ramp(buf: &mut [f32], channels: usize, from: usize, len: usize, up: bool) {
    let len = len.min(SPLICE_FADE_FRAMES);
    if len == 0 {
        return;
    }
    for f in 0..len {
        let t = (f + 1) as f32 / len as f32;
        let g = if up { t } else { 1.0 - t };
        for s in &mut buf[(from + f) * channels..(from + f + 1) * channels] {
            *s *= g;
        }
    }
}

/// A splice reads its span plus a fade on each side.
fn splice_samples(channels: usize) -> usize {
    (MAX_SPLICE_FRAMES + 2 * SPLICE_FADE_FRAMES) * channels
}

/// Holds a resampler chunk being gathered, or a splice's join, plus the rest
/// of the chunk the splice landed in.
fn staging_samples(channels: usize) -> usize {
    (RESAMPLE_CHUNK + MAX_SPLICE_FRAMES + 2 * SPLICE_FADE_FRAMES) * channels + 8
}

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
            self.head = if self.head + 1 == cap {
                0
            } else {
                self.head + 1
            };
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
            self.tail = if self.tail + 1 == cap {
                0
            } else {
                self.tail + 1
            };
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
/// interleaved `out_buf` of `block_frames * node_channels` that downstream
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
    /// Samples removed by design and never played: the startup backlog and
    /// the one startup depth correction, and audio that arrived late for time
    /// already played as silence.
    pub trimmed: Arc<AtomicU64>,
    /// Samples actually read out of the source ring.
    pub consumed: Arc<AtomicU64>,
    /// Ring occupancy (samples) at the end of the last `fill_block`. A gauge,
    /// not a counter -- plain `store`, no accumulation.
    pub level: Arc<AtomicU64>,
    /// Smoothed frames queued ahead of the graph: the latency this source
    /// adds. A gauge.
    pub queue_frames: Arc<AtomicU64>,
    /// Resampler chunks that failed and were dropped.
    pub failed: Arc<AtomicU64>,
    /// Set once the source has delivered audio; the tick thread logs it.
    pub online: Arc<AtomicBool>,
}

impl SourceStats {
    fn new() -> Self {
        Self {
            xrun: Arc::new(AtomicU64::new(0)),
            stalled: Arc::new(AtomicU64::new(0)),
            trimmed: Arc::new(AtomicU64::new(0)),
            consumed: Arc::new(AtomicU64::new(0)),
            level: Arc::new(AtomicU64::new(0)),
            queue_frames: Arc::new(AtomicU64::new(0)),
            failed: Arc::new(AtomicU64::new(0)),
            online: Arc::new(AtomicBool::new(false)),
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
    /// Owning output id (`MONITOR_KEY` for the monitor), the other half of
    /// the key that disambiguates one input feeding several outputs.
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
    pub block_frames: usize,
    pub channels: usize,
    pub io: Option<super::output::SpeakerIo>,
}

struct SourceState {
    label: String,
    channels: usize,
    frames: usize,
    consumer: Consumer<f32>,
    resampler: Option<MultiResampler>,
    input_staging: Vec<f32>,
    /// Holds a splice's crossfaded join until the refill path picks it up.
    splice_tmp: Vec<f32>,
    /// Reads a live source whose producer runs on another clock, absorbing
    /// the drift in its ratio. Replaces `resampler`; `None` for file sources
    /// and for sources on this output's own clock.
    asrc: Option<Box<Asrc>>,
    out_pending: StagingRing,
    chunk_tmp: Vec<f32>,
    out_buf: Vec<f32>,
    input_samples_per_block: usize,
    /// Keeps a live source's queue at its measured headroom. `None` for file
    /// sources, which are paced by backpressure and must not lose audio.
    cushion: Option<Cushion>,
    /// The captured input this source reads; `None` for a ring-source.
    input_id: Option<String>,
    /// Smoothed frames left queued after each read: this source's latency.
    queue_avg: f64,
    /// Frames queued when the previous block finished; what the queue holds
    /// beyond it at the next block is what the source delivered in between.
    queued_after: usize,
    /// >STALL_THRESHOLD since last pop => zero-fill and stop waiting on this source.
    last_pop_at: Instant,
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
    /// Built empty, to take over the running source it replaces (its ring,
    /// queue and resamplers) when the graph swaps in.
    carry_over: bool,
}

impl SourceState {
    /// Takes over `old`'s ring and everything queued or learned about it, so
    /// the source plays on across a graph swap as if nothing happened. What
    /// belongs to the graph (taps, stats, labels) stays this graph's own.
    fn take_over(&mut self, old: &mut SourceState) {
        std::mem::swap(&mut self.consumer, &mut old.consumer);
        std::mem::swap(&mut self.resampler, &mut old.resampler);
        // Whether the source reads through the drift resampler is this graph's
        // decision: the output may have moved to another clock since.
        if self.asrc.is_some() == old.asrc.is_some() {
            std::mem::swap(&mut self.asrc, &mut old.asrc);
        }
        std::mem::swap(&mut self.input_staging, &mut old.input_staging);
        std::mem::swap(&mut self.splice_tmp, &mut old.splice_tmp);
        std::mem::swap(&mut self.out_pending, &mut old.out_pending);
        std::mem::swap(&mut self.chunk_tmp, &mut old.chunk_tmp);
        // Only the fixed-rate path queues resampled output; the drift
        // resampler never reads it.
        if self.asrc.is_some() {
            self.out_pending.clear();
        }
        std::mem::swap(&mut self.cushion, &mut old.cushion);
        self.queue_avg = old.queue_avg;
        self.queued_after = old.queued_after;
        self.last_pop_at = old.last_pop_at;
        self.last_drain_gen = old.last_drain_gen;
        self.carry_over = false;
    }

    fn is_stalled(&self) -> bool {
        self.last_pop_at.elapsed() > STALL_THRESHOLD
    }

    /// Taps are filled at the end of `fill_block`, so an early return would
    /// leave them looping their last block -- a buzz at the block rate.
    fn silence(&mut self) {
        self.out_buf.fill(0.0);
        for (_, buf) in self.handle_bufs.iter_mut() {
            buf.fill(0.0);
        }
    }

    fn fill_block(&mut self, now: f64) {
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
        let was_primed = self.cushion.as_ref().is_some_and(Cushion::is_primed);
        if self.cushion.is_some() && !self.regulate() {
            // Filling before the first read, or refilling after running dry:
            // silence that is not an underrun.
            if self.is_stalled() {
                self.stats
                    .stalled
                    .fetch_add(self.out_buf.len() as u64, Ordering::Relaxed);
            }
            self.silence();
            self.publish_queue();
            return;
        }
        // Coming out of silence: the first samples ramp up rather than step.
        let fade_in = self.cushion.is_some() && !was_primed;
        if self.asrc.is_some() {
            self.fill_resampled(now, fade_in);
        } else {
            self.fill_direct(fade_in);
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
                    for f in 0..self.frames {
                        buf[f * 2] = self.out_buf[f * w + c0];
                        buf[f * 2 + 1] = self.out_buf[f * w + c1];
                    }
                } else {
                    let c = parse_ch(h).map(|k| (k - 1).min(w - 1)).unwrap_or(0);
                    for f in 0..self.frames {
                        buf[f] = self.out_buf[f * w + c];
                    }
                }
            }
        }
        self.stats
            .level
            .store(self.consumer.slots() as u64, Ordering::Relaxed);
        self.publish_queue();
    }

    /// Reads the ring straight, or through the fixed-rate resampler.
    fn fill_direct(&mut self, fade_in: bool) {
        let need = self.out_buf.len();
        let mut written = self.out_pending.pop_into(&mut self.out_buf[..]);
        while written < need {
            self.try_refill_one_chunk(need - written);
            if self.out_pending.len() == 0 {
                // Ring empty too -- zero-fill the rest (real underrun).
                for s in &mut self.out_buf[written..] {
                    *s = 0.0;
                }
                // A stalled/paused source silences by design; only a source that
                // is actively streaming and ran dry mid-block is a real xrun.
                let missing = need - written;
                if self.is_stalled() {
                    self.stats
                        .stalled
                        .fetch_add(missing as u64, Ordering::Relaxed);
                } else {
                    self.stats.xrun.fetch_add(missing as u64, Ordering::Relaxed);
                    if let Some(c) = &mut self.cushion {
                        c.underrun(missing / self.channels);
                    }
                }
                break;
            }
            let n = self.out_pending.pop_into(&mut self.out_buf[written..]);
            written += n;
        }
        if self.cushion.is_some() {
            let w = self.channels;
            if fade_in {
                ramp(&mut self.out_buf, w, 0, written / w, true);
            }
            if written < need {
                // Running dry mid-block: the audio fades into the silence
                // instead of stopping on a step.
                let frames = written / w;
                let len = frames.min(SPLICE_FADE_FRAMES);
                ramp(&mut self.out_buf, w, frames - len, len, false);
            }
        }
    }

    /// Reads the ring through the drift-steered resampler. Fades are applied
    /// to its input, so they pass through the filter like any other audio.
    fn fill_resampled(&mut self, now: f64, fade_in: bool) {
        let queued = self.queued_frames();
        let (target, start, settled) = self.cushion.as_ref().map_or((queued, queued, true), |c| {
            (c.target(), c.start_level(), c.is_settled())
        });
        let Some(asrc) = self.asrc.as_mut() else {
            return;
        };
        let w = self.channels;
        if fade_in || !settled {
            asrc.restart();
        }
        if settled {
            asrc.steer(queued, target, start, now);
        }
        let need = asrc.need() * w;
        asrc.input.clear();
        // A splice staged its joined frames ahead of the ring.
        let staged = self.input_staging.len().min(need);
        asrc.input.extend_from_slice(&self.input_staging[..staged]);
        self.input_staging.drain(..staged);
        let avail = self.consumer.slots().min(need - staged);
        let avail = avail - avail % w;
        if avail > 0 {
            if let Ok(chunk) = self.consumer.read_chunk(avail) {
                let (first, second) = chunk.as_slices();
                asrc.input.extend_from_slice(first);
                asrc.input.extend_from_slice(second);
                chunk.commit_all();
                self.stats
                    .consumed
                    .fetch_add(avail as u64, Ordering::Relaxed);
                self.last_pop_at = Instant::now();
            }
        }
        let got = asrc.input.len();
        if got > 0 {
            self.stats.online.store(true, Ordering::Relaxed);
        }
        if fade_in {
            ramp(&mut asrc.input, w, 0, got / w, true);
        }
        if got < need {
            let frames = got / w;
            let len = frames.min(SPLICE_FADE_FRAMES);
            ramp(&mut asrc.input, w, frames - len, len, false);
            asrc.input.resize(need, 0.0);
            let missing = need - got;
            if self.last_pop_at.elapsed() > STALL_THRESHOLD {
                self.stats
                    .stalled
                    .fetch_add(missing as u64, Ordering::Relaxed);
            } else {
                self.stats.xrun.fetch_add(missing as u64, Ordering::Relaxed);
                if let Some(c) = &mut self.cushion {
                    c.underrun(missing / w);
                }
            }
        }
        match asrc.process() {
            Ok(out) if out.len() == self.out_buf.len() => self.out_buf.copy_from_slice(out),
            _ => {
                self.stats.failed.fetch_add(1, Ordering::Relaxed);
                self.out_buf.fill(0.0);
            }
        }
    }

    /// Frames waiting ahead of the graph: the ring plus anything already
    /// pulled out of it but not yet played (in the ring's own rate).
    fn queued_frames(&self) -> usize {
        (self.consumer.slots() + self.input_staging.len() + self.out_pending.len()) / self.channels
    }

    fn publish_queue(&mut self) {
        self.queued_after = self.queued_frames();
        let filter = self.asrc.as_ref().map_or(0, |a| a.delay_frames());
        let queued = (self.queued_after + filter) as f64;
        self.queue_avg += (queued - self.queue_avg) * 0.02;
        self.stats
            .queue_frames
            .store(self.queue_avg.round() as u64, Ordering::Relaxed);
    }

    /// Holds the queue at the cushion's target. False while priming: the
    /// caller plays silence this block.
    fn regulate(&mut self) -> bool {
        let Some(c) = self.cushion.as_mut() else {
            return true;
        };
        let queued = (self.consumer.slots() + self.input_staging.len() + self.out_pending.len())
            / self.channels;
        // Taken before any splice or discard this block, against where the
        // previous block left the queue: only the source moves it in between.
        c.arrived(queued.saturating_sub(self.queued_after));
        if !c.is_primed() {
            match c.prime(queued) {
                None => return false,
                Some(excess) => self.discard(excess),
            }
        }
        let queued = self.queued_frames();
        let fade = SPLICE_FADE_FRAMES * self.channels;
        if let Some(Adjust::Drop(n)) = self.cushion.as_mut().map(|c| c.observe(queued)) {
            // The crossfade overlaps a fade's worth on each side of the cut
            // into one, so that much of the `n` goes with it.
            let fade = fade.min(n * self.channels);
            self.splice_trim(n * self.channels - fade, fade);
        }
        true
    }

    /// Drops `frames` of backlog outright. Only for audio nobody has heard:
    /// the startup backlog and what piled up while a source was refilling.
    fn discard(&mut self, frames: usize) {
        let samples = (frames * self.channels).min(self.consumer.slots());
        let samples = samples - samples % self.channels;
        if samples == 0 {
            return;
        }
        if let Ok(chunk) = self.consumer.read_chunk(samples) {
            chunk.commit_all();
            self.stats
                .consumed
                .fetch_add(samples as u64, Ordering::Relaxed);
            self.stats
                .trimmed
                .fetch_add(samples as u64, Ordering::Relaxed);
            self.last_pop_at = Instant::now();
        }
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
        self.last_pop_at = Instant::now();
    }

    /// `want` is how many samples the block still needs. Without a resampler
    /// exactly that is taken, so nothing sits pulled out of the ring unplayed.
    fn try_refill_one_chunk(&mut self, want: usize) {
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
            if rs
                .process_chunk(&self.input_staging[..needed], &mut self.chunk_tmp)
                .is_err()
            {
                self.stats.failed.fetch_add(1, Ordering::Relaxed);
                self.input_staging.drain(..needed);
                return;
            }
            self.input_staging.drain(..needed);
        } else {
            self.chunk_tmp.clear();
            let mut want = want.min(RESAMPLE_CHUNK * self.channels);
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
            self.stats.online.store(true, Ordering::Relaxed);
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
    // The clock tells that reader when each block was written.
    taps: Vec<(Producer<f32>, Arc<WriteClock>)>,
}

impl EffectState {
    /// Run the effect chain over `out_buf`. Width <= 2 processes in place; wider
    /// buffers are split into stereo pairs, each through its own instance so
    /// per-channel filter state never bleeds across pairs.
    fn run(&mut self, frames: usize) {
        // A carry-over placeholder whose running node never arrived.
        if self.effects.is_empty() {
            self.out_buf.fill(0.0);
            return;
        }
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
    fn process(&mut self, now: f64) {
        self.receiver.mix_block_at(&mut self.out_buf, now);
        for ((_, buf), key) in self.handle_bufs.iter_mut().zip(&self.wire_keys) {
            match key {
                TapKey::Channel(k) => self.receiver.channel(k, buf),
                TapKey::PrefixMix(p) => self.receiver.prefix_mix(p, buf),
            }
        }
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
    handle
        .strip_prefix("ch")
        .and_then(|s| s.parse::<usize>().ok())
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
    handle
        .strip_prefix("st")
        .and_then(|s| s.parse::<usize>().ok())
}

/// Channel width an edge actually carries. A `chK`/`stA` source handle taps a
/// slice of its node, so the node's own width would be wrong.
fn edge_channels(
    nodes: &[DagNode],
    node_channels: &[usize],
    idx: usize,
    source_handle: Option<&str>,
    frames: usize,
) -> usize {
    match source_handle {
        Some(h) if tap_handle_width(h).is_some() => {
            nodes[idx].out_buf_for_handle(Some(h)).len() / frames
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
/// left untouched). Widths are inferred from length: `len / frames`.
#[inline]
fn add_mapped(src: &[f32], dst: &mut [f32], frames: usize) {
    let src_ch = src.len() / frames;
    let dst_ch = dst.len() / frames;
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
        for f in 0..frames {
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
        for f in 0..frames {
            let v = src[f];
            let db = f * dst_ch;
            for c in 0..dst_ch {
                dst[db + c] += v;
            }
        }
        return;
    }
    let n = src_ch.min(dst_ch);
    for f in 0..frames {
        let sb = f * src_ch;
        let db = f * dst_ch;
        for c in 0..n {
            dst[db + c] += src[sb + c];
        }
    }
}

/// Add `src` (downmixed to mono) into a single physical channel `ch` of `dst`.
#[inline]
fn add_to_channel(src: &[f32], dst: &mut [f32], ch: usize, frames: usize) {
    let src_ch = src.len() / frames;
    let dst_ch = dst.len() / frames;
    if src_ch == 0 || ch >= dst_ch {
        return;
    }
    let g = 1.0 / src_ch as f32;
    for f in 0..frames {
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
fn add_block_at(src: &[f32], dst: &mut [f32], off: usize, frames: usize) {
    let src_ch = src.len() / frames;
    let dst_ch = dst.len() / frames;
    if src_ch == 0 || off >= dst_ch {
        return;
    }
    let n = src_ch.min(dst_ch - off);
    for f in 0..frames {
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
    // A join shorter than a fade still has to end fully on the incoming
    // audio; a single frame is all incoming.
    let span = (dst.len() / channels.max(1)).saturating_sub(1) as f32;
    for (i, s) in first.iter().chain(second.iter()).enumerate() {
        if i >= dst.len() {
            break;
        }
        let w = if span == 0.0 {
            1.0
        } else {
            ((i / channels) as f32 / span).min(1.0)
        };
        dst[i] = dst[i] * (1.0 - w) + s * w;
    }
}

struct DelayLine {
    buf: Box<[f32]>,
    scratch: Box<[f32]>,
    pos: usize,
}

fn delay_len(d: &Option<DelayLine>) -> usize {
    d.as_ref().map_or(0, |d| d.buf.len())
}

/// Whether two nodes are fed by the same edges from the same nodes, at the
/// same delays.
fn same_edges(
    new_edges: &[IncomingEdge],
    new_ids: &[String],
    old_edges: &[IncomingEdge],
    old_ids: &[String],
) -> bool {
    new_edges.len() == old_edges.len()
        && new_edges.iter().all(|e| {
            old_edges.iter().any(|o| {
                old_ids[o.src_idx] == new_ids[e.src_idx]
                    && o.source_handle == e.source_handle
                    && o.target_handle == e.target_handle
                    && delay_len(&o.delay) == delay_len(&e.delay)
            })
        })
}

/// Hands each delay line of `old_edges` to the edge of `new_edges` that runs
/// between the same nodes and handles at the same delay, so the audio it holds
/// keeps flowing instead of restarting from silence.
fn carry_delays(
    new_edges: &mut [IncomingEdge],
    new_ids: &[String],
    old_edges: &mut [IncomingEdge],
    old_ids: &[String],
) {
    for e in new_edges.iter_mut() {
        let Some(d) = e.delay.as_mut() else { continue };
        let src = &new_ids[e.src_idx];
        if let Some(od) = old_edges
            .iter_mut()
            .filter(|o| {
                &old_ids[o.src_idx] == src
                    && o.source_handle == e.source_handle
                    && o.target_handle == e.target_handle
            })
            .find_map(|o| o.delay.as_mut().filter(|od| od.same_shape(d)))
        {
            std::mem::swap(d, od);
        }
    }
}

impl DelayLine {
    fn same_shape(&self, other: &DelayLine) -> bool {
        self.buf.len() == other.buf.len() && self.scratch.len() == other.scratch.len()
    }

    fn new(delay_frames: usize, channels: usize, block_frames: usize) -> Self {
        Self {
            buf: vec![0.0; delay_frames * channels].into_boxed_slice(),
            scratch: vec![0.0; block_frames * channels].into_boxed_slice(),
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
    /// Frames `process_block` renders per call; every node buffer holds one.
    block_frames: usize,
    /// Interleaved channel width of `process_block`'s output. Stereo unless a
    /// speaker sets it to the device's channel count.
    out_channels: usize,
    nodes: Vec<DagNode>,
    /// Graph node id of each of `nodes` (empty for a wire sender's sink).
    node_ids: Vec<String>,
    /// What each node was built from, where a later graph may carry the node
    /// over: equal keys build equal nodes.
    node_keys: Vec<Option<String>>,
    /// Whether each node is heard: it reaches a terminal or a wire sink.
    audible: Vec<bool>,
    terminals: Vec<TerminalEdge>,
    /// Lookahead the graph's delay compensation has aligned every path to: the
    /// deepest cumulative effect latency from any source to this output. The
    /// whole mix is delayed by this, so it is the graph's own latency.
    latency_frames: usize,
    /// Blocks produced by `process_block`. A clone lives in this build's
    /// `BuiltOutputGraph::output` so the non-RT tick thread can compare this
    /// worker's real block rate against `sample_rate / block_frames`.
    blocks: Arc<AtomicU64>,
}

impl OutputGraph {
    pub(super) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(super) fn block_frames(&self) -> usize {
        self.block_frames
    }

    pub(super) fn out_channels(&self) -> usize {
        self.out_channels
    }

    pub(super) fn latency_frames(&self) -> usize {
        self.latency_frames
    }

    pub(super) fn set_out_channels(&mut self, channels: usize) {
        self.out_channels = channels;
    }

    pub(super) fn active_output_channels(&self) -> usize {
        self.terminals
            .iter()
            .map(|terminal| match terminal.route {
                Some((offset, width)) => offset + width,
                None => {
                    self.nodes[terminal.src_idx]
                        .out_buf_for_handle(terminal.source_handle.as_deref())
                        .len()
                        / self.block_frames
                }
            })
            .max()
            .unwrap_or(1)
            .clamp(1, self.out_channels)
    }

    /// Marks the sources reading `inputs` as running on this output's clock:
    /// they have no drift to absorb and read without the drift resampler.
    /// Call before the graph goes live.
    pub(super) fn lock_inputs(&mut self, inputs: &HashSet<String>) {
        for node in &mut self.nodes {
            let DagNode::Source(s) = node else { continue };
            if s.input_id.as_ref().is_some_and(|id| inputs.contains(id)) {
                s.asrc = None;
            }
        }
    }

    /// Gives the sources reading `input` its producer's write timing, which
    /// the drift loop reads the queue against.
    pub(super) fn attach_write_clock(&mut self, input: &str, clock: &Arc<WriteClock>) {
        for node in &mut self.nodes {
            let DagNode::Source(s) = node else { continue };
            if s.input_id.as_deref() != Some(input) {
                continue;
            }
            if let Some(asrc) = &mut s.asrc {
                asrc.set_clock(clock.clone());
            }
        }
    }

    /// Attach a publish ring to a fan-out effect node; its `out_buf` is pushed
    /// there each block for another output's ring-source to read.
    pub(super) fn attach_tap(
        &mut self,
        node_idx: usize,
        prod: Producer<f32>,
        clock: Arc<WriteClock>,
    ) {
        if let Some(DagNode::Effect(e)) = self.nodes.get_mut(node_idx) {
            e.taps.push((prod, clock));
        }
    }

    /// RT-safe. Whether swapping this graph in for `old` changes nothing that
    /// is heard: every node that reaches the output is carried over from
    /// `old` and fed by the same edges at the same delays, and nothing heard
    /// in `old` is gone. Call before `adopt_from`.
    pub(super) fn plays_like(&self, old: &OutputGraph) -> bool {
        let heard = |g: &OutputGraph| g.audible.iter().filter(|&&a| a).count();
        if heard(self) != heard(old) || self.terminals.len() != old.terminals.len() {
            return false;
        }
        for i in (0..self.nodes.len()).filter(|&i| self.audible[i]) {
            let Some(j) = (0..old.nodes.len()).find(|&j| {
                old.audible[j]
                    && !self.node_ids[i].is_empty()
                    && old.node_ids[j] == self.node_ids[i]
            }) else {
                return false;
            };
            if self.node_keys[i].is_none() || old.node_keys[j] != self.node_keys[i] {
                return false;
            }
            let same = match (&self.nodes[i], &old.nodes[j]) {
                (DagNode::Source(new), DagNode::Source(_)) => new.carry_over,
                (DagNode::Effect(new), DagNode::Effect(prev)) => {
                    new.effects.is_empty()
                        && same_edges(&new.incoming, &self.node_ids, &prev.incoming, &old.node_ids)
                        && same_edges(
                            &new.sidechain,
                            &self.node_ids,
                            &prev.sidechain,
                            &old.node_ids,
                        )
                }
                _ => false,
            };
            if !same {
                return false;
            }
        }
        self.terminals.iter().all(|t| {
            old.terminals.iter().any(|o| {
                old.node_ids[o.src_idx] == self.node_ids[t.src_idx]
                    && o.source_handle == t.source_handle
                    && o.route == t.route
                    && delay_len(&o.delay) == delay_len(&t.delay)
            })
        })
    }

    /// RT-safe. Takes over the running state of every node this graph was
    /// built to carry over from `old`, the graph it replaces, and every delay
    /// line on an edge both share at the same length. Returns whether any
    /// node was carried over; `old` cannot render after that.
    pub(super) fn adopt_from(&mut self, old: &mut OutputGraph) -> bool {
        let mut carried = false;
        for i in 0..self.nodes.len() {
            let Some(j) = (0..old.nodes.len())
                .find(|&j| !self.node_ids[i].is_empty() && old.node_ids[j] == self.node_ids[i])
            else {
                continue;
            };
            let same_key = self.node_keys[i].is_some() && old.node_keys[j] == self.node_keys[i];
            match (&mut self.nodes[i], &mut old.nodes[j]) {
                (DagNode::Source(new), DagNode::Source(prev)) if same_key && new.carry_over => {
                    new.take_over(prev);
                    carried = true;
                }
                (DagNode::Effect(new), DagNode::Effect(prev)) => {
                    if same_key && new.effects.is_empty() {
                        std::mem::swap(&mut new.effects, &mut prev.effects);
                        carried = true;
                    }
                    carry_delays(
                        &mut new.incoming,
                        &self.node_ids,
                        &mut prev.incoming,
                        &old.node_ids,
                    );
                    carry_delays(
                        &mut new.sidechain,
                        &self.node_ids,
                        &mut prev.sidechain,
                        &old.node_ids,
                    );
                }
                (DagNode::Consumer(new), DagNode::Consumer(prev)) => {
                    carry_delays(
                        &mut new.incoming,
                        &self.node_ids,
                        &mut prev.incoming,
                        &old.node_ids,
                    );
                }
                _ => {}
            }
        }
        for t in self.terminals.iter_mut() {
            let Some(d) = t.delay.as_mut() else { continue };
            let src = &self.node_ids[t.src_idx];
            if let Some(od) = old
                .terminals
                .iter_mut()
                .filter(|o| {
                    &old.node_ids[o.src_idx] == src
                        && o.source_handle == t.source_handle
                        && o.route == t.route
                })
                .find_map(|o| o.delay.as_mut().filter(|od| od.same_shape(d)))
            {
                std::mem::swap(d, od);
            }
        }
        carried
    }

    /// Fill `output` (`block_frames * out_channels` long) with one block of
    /// mixed audio at `sample_rate`.
    pub(super) fn process_block(&mut self, output: &mut [f32]) {
        self.process_block_at(output, now_secs());
    }

    /// `process_block` at `now` seconds on `input_bridge::now_secs`'s clock.
    pub(super) fn process_block_at(&mut self, output: &mut [f32], now: f64) {
        let frames = self.block_frames;
        self.blocks.fetch_add(1, Ordering::Relaxed);
        for node in &mut self.nodes {
            match node {
                DagNode::Source(s) => s.fill_block(now),
                DagNode::Producer(p) => p.process(now),
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
                    add_mapped(src, buf, frames);
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
                        Some((off, 1)) => add_to_channel(src, &mut eff.out_buf, off, frames),
                        Some((off, _)) => add_block_at(src, &mut eff.out_buf, off, frames),
                        None => add_mapped(src, &mut eff.out_buf, frames),
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
                        add_mapped(src, sc_buf, frames);
                    }
                }
                if !eff.bypass.load(Ordering::Relaxed) {
                    eff.run(frames);
                }
                let w = eff.out_buf.len() / frames;
                for (h, buf) in eff.handle_bufs.iter_mut() {
                    if let Some(a) = parse_stereo(h) {
                        let c0 = (a - 1).min(w - 1);
                        let c1 = a.min(w - 1);
                        for f in 0..frames {
                            buf[f * 2] = eff.out_buf[f * w + c0];
                            buf[f * 2 + 1] = eff.out_buf[f * w + c1];
                        }
                    } else if let Some(k) = parse_ch(h) {
                        let c = (k - 1).min(w - 1);
                        for f in 0..frames {
                            buf[f] = eff.out_buf[f * w + c];
                        }
                    }
                }
                // Publish the processed block to every consuming output's ring.
                for (prod, clock) in eff.taps.iter_mut() {
                    clock.record(eff.out_buf.len(), now);
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
                Some((off, 1)) => add_to_channel(src, output, off, frames),
                Some((off, _)) => add_block_at(src, output, off, frames),
                None => add_mapped(src, output, frames),
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
    /// Latency and working block of every effect built here.
    pub node_timings: Vec<NodeTiming>,
    /// What the next build of this output needs to carry these nodes over.
    pub carried: HashMap<String, CarriedNode>,
    /// Inputs whose source this graph takes over from the one it replaces:
    /// their rings, and the bridges feeding them, stay as they are.
    pub carried_inputs: Vec<String>,
}

/// A node as a later build of the same output sees it: equal keys mean the
/// node can be carried over rather than built again, and the rest is what
/// building it produced that the placeholder must reproduce.
#[derive(Clone)]
pub(super) struct CarriedNode {
    key: String,
    effect: Option<CarriedEffect>,
}

#[derive(Clone)]
struct CarriedEffect {
    latency: usize,
    working_block: Option<usize>,
    full_width: bool,
    bypass: Arc<AtomicBool>,
    meter: Option<MeterHandle>,
    lufs: Option<LufsHandle>,
    gr: Option<GrHandle>,
    scope: Option<WaveformHandle>,
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
    block_frames: usize,
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
    mut cut_leaves: HashMap<String, CutLeaf>,
    // What the graph this build replaces is running, for carrying unchanged
    // nodes over. Empty when nothing is being replaced in place.
    previous: &HashMap<String, CarriedNode>,
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
    let mut node_timings: Vec<NodeTiming> = Vec::new();
    let mut carried: HashMap<String, CarriedNode> = HashMap::new();
    let mut carried_inputs: Vec<String> = Vec::new();
    // Per-node channel width; effects inherit the max width of their upstreams.
    let mut node_channels: Vec<usize> = Vec::with_capacity(topo.len());

    for id in &topo {
        // A fan-out node owned by an earlier output: read its published block
        // from the ring instead of rebuilding the whole upstream chain.
        if let Some((consumer, owner_sr, width, clock)) = cut_leaves.remove(id) {
            let source = ring_source(
                id,
                consumer,
                clock,
                owner_sr,
                output_sr,
                block_frames,
                width,
                realtime,
                valid,
            )?;
            sources.push(SourceMeta {
                label: format!("{} out={}", source.label, output_id.unwrap_or("monitor")),
                stats: source.stats.clone(),
                channels: width,
                native_sr: owner_sr,
                frames_per_block: source.input_samples_per_block / width.max(1),
                input_id: None,
                output_id: output_id.unwrap_or(MONITOR_KEY).to_string(),
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
                    Some(ChannelReceiver::new(receiver.register_consumer(
                        output_sr,
                        block_frames,
                        realtime,
                    )))
                }
                InputSpec::WebRtcRecv {
                    node_id,
                    opus_bitrate,
                    opus_application,
                } => {
                    let session = crate::audio::webrtc::get_or_create(
                        node_id,
                        *opus_bitrate,
                        *opus_application,
                    );
                    Some(ChannelReceiver::new(session.register_bridge(
                        output_sr,
                        block_frames,
                        realtime,
                    )))
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
                    handle_bufs.push((h, vec![0.0; block_frames]));
                }
                id_to_index.insert(id.clone(), nodes.len());
                nodes.push(DagNode::Producer(ProducerState {
                    receiver,
                    out_buf: vec![0.0; block_frames * pw],
                    handle_bufs,
                    wire_keys,
                }));
                node_latencies.push(0);
                node_channels.push(pw);
                continue;
            }
            // File sources are paced by backpressure; dropping backlog plays fast.
            let source_realtime = realtime && !matches!(input.spec, InputSpec::AudioFile { .. });
            let input_sr = *input_native_sr
                .get(id)
                .ok_or_else(|| AppError::Validation(format!("input {id} has no SR")))?;
            let source_channels = input_native_channels.get(id).copied().unwrap_or(2) as usize;
            let key = format!(
                "src|{:?}|{input_sr}|{source_channels}|{output_sr}|{block_frames}|{source_realtime}",
                input.spec
            );
            let carry_over = previous.get(id).is_some_and(|p| p.key == key);
            carried.insert(id.clone(), CarriedNode { key, effect: None });
            // Scale by channels to keep the buffered span constant in time; at
            // high channel counts a smaller cushion starves on capture-clock drift.
            let consumer = if carry_over {
                carried_inputs.push(id.clone());
                RingBuffer::<f32>::new(1).1
            } else {
                let (producer, consumer) =
                    RingBuffer::<f32>::new(ring_capacity_frames(input_sr) * source_channels);
                producer_pairs.push((id.clone(), producer));
                consumer
            };
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
                    (h, vec![0.0; block_frames * w])
                })
                .collect();
            let resampler = if input_sr == output_sr {
                None
            } else {
                Some(MultiResampler::new(
                    input_sr,
                    output_sr,
                    RESAMPLE_CHUNK,
                    source_channels,
                )?)
            };
            let out_max = resampler
                .as_ref()
                .map(|r| r.out_max())
                .unwrap_or(RESAMPLE_CHUNK);
            // x4 headroom: one chunk draining + one in-flight + alignment slack.
            let staging_cap = (out_max * 4 + block_frames) * source_channels;
            let input_frames_per_block =
                (block_frames as u64 * input_sr as u64 + output_sr as u64 - 1) / output_sr as u64;
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
            let label = format!(
                "{kind}@{input_sr}->{output_sr} out={}",
                output_id.unwrap_or("monitor")
            );
            let stats = SourceStats::new();
            sources.push(SourceMeta {
                label: label.clone(),
                stats: stats.clone(),
                channels: source_channels,
                native_sr: input_sr,
                frames_per_block: input_frames_per_block as usize,
                input_id: Some(id.clone()),
                output_id: output_id.unwrap_or(MONITOR_KEY).to_string(),
                capture: None,
            });
            let source = SourceState {
                label,
                channels: source_channels,
                frames: block_frames,
                consumer,
                resampler,
                input_staging: Vec::with_capacity(staging_samples(source_channels)),
                splice_tmp: Vec::with_capacity(splice_samples(source_channels)),
                asrc: source_realtime
                    .then(|| Asrc::new(input_sr, output_sr, block_frames, source_channels))
                    .transpose()?
                    .map(Box::new),
                out_pending: StagingRing::with_capacity(staging_cap),
                chunk_tmp: Vec::with_capacity(out_max * source_channels),
                out_buf: vec![0.0; block_frames * source_channels],
                input_samples_per_block,
                cushion: source_realtime
                    .then(|| Cushion::new(input_frames_per_block as usize, input_sr)),
                queue_avg: 0.0,
                queued_after: 0,
                input_id: Some(id.clone()),
                last_pop_at: Instant::now(),
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
                carry_over,
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
                .map(|(i, sh, _)| {
                    edge_channels(&nodes, &node_channels, *i, sh.as_deref(), block_frames)
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
            let eff_channels = upstream_w.max(target_w).max(tap_w).max(1);
            let key = format!(
                "fx|{:?}|{eff_channels}|{output_sr}|{block_frames}|{realtime}",
                super::sig::structural_effect(&effect.spec)
            );
            let previous_effect = previous
                .get(id)
                .filter(|p| p.key == key)
                .and_then(|p| p.effect.clone());
            // A node carried over is built empty and takes over the running
            // instances when the graph swaps in; anything else is built now.
            // Built once the node's width is known: a plugin is offered that
            // width and may take it whole, the way a DAW instantiates one
            // multichannel plugin instead of several stereo ones.
            let (effects, this_effect) = match previous_effect {
                Some(p) => (Vec::new(), p),
                None => {
                    let build = instantiate_effect(
                        &effect.spec,
                        id,
                        output_sr,
                        block_frames,
                        realtime,
                        true,
                        eff_channels,
                        registry,
                    );
                    if let Some(c) = build.control {
                        controls.push((id.clone(), c));
                    }
                    if build.bypass_is_new {
                        bypasses.push((id.clone(), build.bypass.clone()));
                    }
                    // Analyzers read all channels at once, and so does a plugin
                    // that accepted the node's full width. Everything else runs
                    // one instance per stereo pair.
                    let full_width = build.full_width
                        || matches!(
                            effect.spec,
                            EffectSpec::LevelMeter(_)
                                | EffectSpec::Waveform(_)
                                | EffectSpec::Spectrum(_)
                        );
                    let this_effect = CarriedEffect {
                        latency: build.effect.latency_frames(),
                        working_block: build.effect.working_block(),
                        full_width,
                        bypass: build.bypass,
                        meter: build.meter,
                        lufs: build.lufs,
                        gr: build.gr,
                        scope: build.scope,
                    };
                    let pairs = if full_width {
                        1
                    } else {
                        eff_channels.div_ceil(2)
                    };
                    let mut effects = Vec::with_capacity(pairs);
                    effects.push(build.effect);
                    for _ in 1..pairs {
                        // Extra stereo pairs are separate instances for wider
                        // audio, never the editor target. Extra pairs exist
                        // only when the node is driven pairwise, so each is
                        // asked for stereo rather than the node's full width.
                        let extra = instantiate_effect(
                            &effect.spec,
                            id,
                            output_sr,
                            block_frames,
                            realtime,
                            false,
                            2,
                            registry,
                        );
                        effects.push(extra.effect);
                    }
                    (effects, this_effect)
                }
            };
            meters.extend(this_effect.meter.clone());
            lufs.extend(this_effect.lufs.clone());
            gr_handles.extend(this_effect.gr.clone());
            scopes.extend(this_effect.scope.clone());
            let bypass = this_effect.bypass.clone();
            let make_edge =
                |src_idx: usize, source_handle: Option<String>, target_handle: Option<String>| {
                    let pad = max_upstream - node_latencies[src_idx];
                    let width = edge_channels(
                        &nodes,
                        &node_channels,
                        src_idx,
                        source_handle.as_deref(),
                        block_frames,
                    );
                    IncomingEdge {
                        src_idx,
                        source_handle,
                        target_handle,
                        delay: if pad > 0 {
                            Some(DelayLine::new(pad, width, block_frames))
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
                Some(vec![0.0; block_frames * eff_channels])
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
                    (h, vec![0.0; block_frames * w])
                })
                .collect();
            let own = this_effect.latency;
            node_timings.push(NodeTiming {
                node_id: id.clone(),
                latency_frames: own as u32,
                working_block: this_effect.working_block.map(|w| w as u32),
            });
            let full_width = this_effect.full_width;
            carried.insert(
                id.clone(),
                CarriedNode {
                    key,
                    effect: Some(this_effect),
                },
            );
            id_to_index.insert(id.clone(), nodes.len());
            nodes.push(DagNode::Effect(EffectState {
                effects,
                full_width,
                bypass,
                incoming,
                sidechain,
                out_buf: vec![0.0; block_frames * eff_channels],
                sidechain_buf,
                pair_main: vec![0.0; block_frames * 2],
                pair_side: vec![0.0; block_frames * 2],
                handle_bufs,
                taps: Vec::new(),
            }));
            node_meta.insert(id.clone(), (nodes.len() - 1, eff_channels));
            node_latencies.push(max_upstream + own);
            node_channels.push(eff_channels);
        }
    }

    // Matches the source label style (`out=<id>` / "monitor").
    let out_label = output_id
        .map(|id| format!("out={id}"))
        .unwrap_or_else(|| "monitor".to_string());
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
        let max_up = up
            .iter()
            .map(|(i, _, _)| node_latencies[*i])
            .max()
            .unwrap_or(0);
        let incoming: Vec<IncomingEdge> = up
            .into_iter()
            .map(|(idx, source_handle, target_handle)| {
                let pad = max_up - node_latencies[idx];
                let width = edge_channels(
                    &nodes,
                    &node_channels,
                    idx,
                    source_handle.as_deref(),
                    block_frames,
                );
                IncomingEdge {
                    src_idx: idx,
                    source_handle,
                    target_handle,
                    delay: if pad > 0 {
                        Some(DelayLine::new(pad, width, block_frames))
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
            channel_bufs.push((format!("ch{c}"), vec![0.0; block_frames]));
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
                    output_sr,
                );
                sender.set_send_consumers(send_consumers);
            }
            OutputSpec::WebRtcSend {
                node_id,
                opus_bitrate,
                opus_application,
                ..
            } => {
                let session =
                    crate::audio::webrtc::get_or_create(node_id, *opus_bitrate, *opus_application);
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

        let (node_ids, node_keys) = node_identities(nodes.len(), &id_to_index, &carried);
        let audible = audible_nodes(&nodes, &[]);
        return Ok(BuiltOutputGraph {
            graph: OutputGraph {
                sample_rate: output_sr,
                block_frames,
                out_channels: 2,
                nodes,
                node_ids,
                node_keys,
                audible,
                terminals: Vec::new(),
                latency_frames: max_up,
                blocks: blocks.clone(),
            },
            controls,
            bypasses,
            meters,
            lufs,
            gr_handles,
            scopes,
            sources,
            output: OutputMeta {
                label: out_label,
                blocks,
                sample_rate: output_sr,
                block_frames,
                channels: 2,
                io: None,
            },
            node_meta,
            node_timings,
            carried,
            carried_inputs,
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
                    let width = edge_channels(
                        &nodes,
                        &node_channels,
                        src_idx,
                        source_handle.as_deref(),
                        block_frames,
                    );
                    TerminalEdge {
                        src_idx,
                        source_handle,
                        route,
                        delay: if pad > 0 {
                            Some(DelayLine::new(pad, width, block_frames))
                        } else {
                            None
                        },
                    }
                })
                .collect()
        }
        None => Vec::new(),
    };

    let (node_ids, node_keys) = node_identities(nodes.len(), &id_to_index, &carried);
    let audible = audible_nodes(&nodes, &terminals);
    Ok(BuiltOutputGraph {
        graph: OutputGraph {
            sample_rate: output_sr,
            block_frames,
            out_channels: 2,
            nodes,
            node_ids,
            node_keys,
            audible,
            terminals,
            latency_frames: node_latencies.iter().copied().max().unwrap_or(0),
            blocks: blocks.clone(),
        },
        controls,
        bypasses,
        meters,
        lufs,
        gr_handles,
        scopes,
        sources,
        output: OutputMeta {
            label: out_label,
            blocks,
            sample_rate: output_sr,
            block_frames,
            channels: 2,
            io: None,
        },
        node_meta,
        node_timings,
        carried,
        carried_inputs,
    })
}

/// Which nodes reach a terminal or a wire sink. Nodes come in topological
/// order, so walking them backwards visits every consumer before its inputs.
fn audible_nodes(nodes: &[DagNode], terminals: &[TerminalEdge]) -> Vec<bool> {
    let mut audible = vec![false; nodes.len()];
    for t in terminals {
        audible[t.src_idx] = true;
    }
    for i in (0..nodes.len()).rev() {
        let edges: &[IncomingEdge] = match &nodes[i] {
            DagNode::Consumer(c) => {
                audible[i] = true;
                &c.incoming
            }
            DagNode::Effect(e) if audible[i] => {
                for s in &e.sidechain {
                    audible[s.src_idx] = true;
                }
                &e.incoming
            }
            _ => continue,
        };
        for e in edges {
            audible[e.src_idx] = true;
        }
    }
    audible
}

/// Each node's graph id (empty for a node no graph id names) and carry-over
/// key, by index.
fn node_identities(
    len: usize,
    id_to_index: &HashMap<String, usize>,
    carried: &HashMap<String, CarriedNode>,
) -> (Vec<String>, Vec<Option<String>>) {
    let mut ids = vec![String::new(); len];
    for (id, &i) in id_to_index {
        ids[i] = id.clone();
    }
    let keys = ids
        .iter()
        .map(|id| carried.get(id).map(|c| c.key.clone()))
        .collect();
    (ids, keys)
}

/// A fan-out node's published ring as another output reads it: the ring, the
/// owner's rate and width, and when the owner wrote to it.
pub(super) type CutLeaf = (Consumer<f32>, u32, usize, Arc<WriteClock>);

/// Builds a `SourceState` that reads a fan-out node's published block from a
/// ring (written at `owner_sr`) and resamples it to this graph's `output_sr`.
/// Reuses the source machinery so per-channel taps and backlog-dropping behave
/// exactly like a captured input.
#[allow(clippy::too_many_arguments)]
fn ring_source(
    id: &str,
    consumer: Consumer<f32>,
    clock: Arc<WriteClock>,
    owner_sr: u32,
    output_sr: u32,
    block_frames: usize,
    channels: usize,
    realtime: bool,
    valid: &ValidGraph,
) -> AppResult<SourceState> {
    let resampler = if owner_sr == output_sr {
        None
    } else {
        Some(MultiResampler::new(
            owner_sr,
            output_sr,
            RESAMPLE_CHUNK,
            channels,
        )?)
    };
    let out_max = resampler
        .as_ref()
        .map(|r| r.out_max())
        .unwrap_or(RESAMPLE_CHUNK);
    let staging_cap = (out_max * 4 + block_frames) * channels;
    let input_frames_per_block =
        (block_frames as u64 * owner_sr as u64 + output_sr as u64 - 1) / output_sr as u64;
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
            (h, vec![0.0; block_frames * w])
        })
        .collect();

    let asrc = if realtime {
        let mut asrc = Asrc::new(owner_sr, output_sr, block_frames, channels)?;
        asrc.set_clock(clock);
        Some(Box::new(asrc))
    } else {
        None
    };

    Ok(SourceState {
        label: format!("cut:{id}"),
        channels,
        frames: block_frames,
        consumer,
        resampler,
        input_staging: Vec::with_capacity(staging_samples(channels)),
        splice_tmp: Vec::with_capacity(splice_samples(channels)),
        asrc,
        out_pending: StagingRing::with_capacity(staging_cap),
        chunk_tmp: Vec::with_capacity(out_max * channels),
        out_buf: vec![0.0; block_frames * channels],
        input_samples_per_block,
        cushion: realtime.then(|| Cushion::new(input_frames_per_block as usize, owner_sr)),
        input_id: None,
        queue_avg: 0.0,
        queued_after: 0,
        last_pop_at: Instant::now(),
        volume: Arc::new(AtomicU32::new(0x3F80_0000)),
        paused: None,
        drain: None,
        last_drain_gen: 0,
        meter: None,
        handle_bufs,
        stats: SourceStats::new(),
        carry_over: false,
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

    for out in owner_order(valid) {
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

/// Order in which outputs claim shared nodes, and so the order they must be
/// built in (an owner wires the rings its consumers read). Speakers go first:
/// an owner computes the node at its own block, and a device-paced graph must
/// not read it late through a ring filled at a timer worker's larger block.
pub(super) fn owner_order(valid: &ValidGraph) -> Vec<&crate::audio::graph::ValidOutput> {
    let mut outputs: Vec<_> = valid.outputs.iter().collect();
    outputs.sort_by_key(|o| !matches!(o.spec, OutputSpec::Speaker { .. }));
    outputs
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

#[allow(dead_code)]
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
    use super::{add_mapped, crossfade_into, DelayLine, SPLICE_FADE_FRAMES, TIMER_BLOCK_FRAMES};

    // Latency compensation on a branch that bypasses a latent effect must be a
    // pure delay: same samples, same order, only shifted.
    #[test]
    fn delay_line_shifts_without_losing_samples() {
        const PAD_FRAMES: usize = 482;
        let mut line = DelayLine::new(PAD_FRAMES, 2, TIMER_BLOCK_FRAMES);
        let mut fed: Vec<f32> = Vec::new();
        let mut got: Vec<f32> = Vec::new();
        for b in 0..4 {
            let mut input = vec![0.0_f32; TIMER_BLOCK_FRAMES * 2];
            for f in 0..TIMER_BLOCK_FRAMES {
                let v = (b * TIMER_BLOCK_FRAMES + f) as f32;
                input[f * 2] = v;
                input[f * 2 + 1] = -v;
            }
            fed.extend_from_slice(&input);
            let mut dst = vec![0.0_f32; TIMER_BLOCK_FRAMES * 2];
            add_mapped(line.delayed(&input), &mut dst, TIMER_BLOCK_FRAMES);
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
        let mut line = DelayLine::new(PAD_FRAMES, 1, TIMER_BLOCK_FRAMES);
        let mut fed: Vec<f32> = Vec::new();
        let mut got: Vec<f32> = Vec::new();
        for b in 0..4 {
            let mut input = vec![0.0_f32; TIMER_BLOCK_FRAMES];
            for (f, s) in input.iter_mut().enumerate() {
                *s = (b * TIMER_BLOCK_FRAMES + f) as f32 + 1.0;
            }
            fed.extend_from_slice(&input);
            let mut dst = vec![0.0_f32; TIMER_BLOCK_FRAMES * 2];
            add_mapped(line.delayed(&input), &mut dst, TIMER_BLOCK_FRAMES);
            got.extend_from_slice(&dst);
        }
        // Mono upmixes to both channels, and no frame of any block stays silent.
        for b in 1..4 {
            for f in 0..TIMER_BLOCK_FRAMES {
                let i = b * TIMER_BLOCK_FRAMES * 2 + f * 2;
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

    // The last splice of the startup correction carries what is left of it,
    // often less than a whole fade; its join must still end on the incoming
    // audio, which is what the stream continues with.
    #[test]
    fn a_short_splice_still_ends_on_the_incoming_audio() {
        const CH: usize = 2;
        for frames in [1, 4, SPLICE_FADE_FRAMES / 2, SPLICE_FADE_FRAMES - 1] {
            let mut dst = vec![1.0_f32; frames * CH];
            let incoming = vec![0.0_f32; frames * CH];
            crossfade_into(&mut dst, &incoming, &[], CH);
            let last = (frames - 1) * CH;
            assert_eq!(dst[last], 0.0, "{frames}-frame join ends at {}", dst[last]);
        }
    }

    #[test]
    fn add_mapped_maps_by_channel() {
        // 4->2: first two channels pass, rest dropped.
        let mut src = vec![0.0; TIMER_BLOCK_FRAMES * 4];
        for f in 0..TIMER_BLOCK_FRAMES {
            for c in 0..4 {
                src[f * 4 + c] = c as f32 + 1.0;
            }
        }
        let mut dst = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        add_mapped(&src, &mut dst, TIMER_BLOCK_FRAMES);
        assert_eq!(dst[0], 1.0);
        assert_eq!(dst[1], 2.0);

        // 2->1: mono downmix is the mean.
        let mut stereo = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        for f in 0..TIMER_BLOCK_FRAMES {
            stereo[f * 2] = 1.0;
            stereo[f * 2 + 1] = 3.0;
        }
        let mut mono = vec![0.0; TIMER_BLOCK_FRAMES];
        add_mapped(&stereo, &mut mono, TIMER_BLOCK_FRAMES);
        assert!((mono[0] - 2.0).abs() < 1e-6);

        // equal width: straight sum-in.
        let src = vec![0.5; TIMER_BLOCK_FRAMES * 3];
        let mut dst = vec![0.25; TIMER_BLOCK_FRAMES * 3];
        add_mapped(&src, &mut dst, TIMER_BLOCK_FRAMES);
        assert!((dst[0] - 0.75).abs() < 1e-6);
    }
}

#[cfg(test)]
pub(super) mod graph_tests {
    use super::*;
    use crate::audio::graph::{EdgeSpec, EffectSpec, GraphSpec, NodeKind, NodeSpec, ValidGraph};

    const SR: u32 = 48_000;

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

    fn gain_node(id: &str, db: f32) -> NodeSpec {
        node(id, NodeKind::Gain, serde_json::json!({ "gainDb": db }))
    }

    fn speaker(id: &str) -> NodeSpec {
        node(
            id,
            NodeKind::Speaker,
            serde_json::json!({ "deviceId": "dev" }),
        )
    }

    fn edge(id: &str, from: &str, sh: Option<&str>, to: &str, th: Option<&str>) -> EdgeSpec {
        EdgeSpec {
            id: id.to_string(),
            source: from.to_string(),
            source_handle: sh.map(str::to_string),
            target: to.to_string(),
            target_handle: th.map(str::to_string),
        }
    }

    /// mic → gain(0 dB) → speaker, validated.
    pub(in crate::audio::pipeline) fn passthrough_graph() -> (ValidGraph, String) {
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m"), gain_node("g", 0.0), speaker("s")],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "g", None, "s", None),
            ],
        };
        (g.validate().expect("valid"), "s".to_string())
    }

    fn stereo_ramp(frames: usize, offset: f32) -> Vec<f32> {
        (0..frames * 2)
            .map(|i| ((i / 2) as f32 + offset) * 0.01)
            .collect()
    }

    fn push_all(prod: &mut Producer<f32>, data: &[f32]) -> usize {
        // The ring is sized for a full second, far more than any test feeds.
        prod.push_partial_slice(data).1.is_empty() as usize * data.len()
    }

    pub(in crate::audio::pipeline) fn fresh_registry() -> EffectRegistry {
        EffectRegistry::new()
    }

    pub(in crate::audio::pipeline) fn build(
        output_id: Option<&str>,
        output_sr: u32,
        valid: &ValidGraph,
        input_sr: u32,
        realtime: bool,
    ) -> (BuiltOutputGraph, HashMap<String, Producer<f32>>) {
        build_with_block(
            output_id,
            output_sr,
            TIMER_BLOCK_FRAMES,
            valid,
            input_sr,
            realtime,
        )
    }

    pub(in crate::audio::pipeline) fn build_with_block(
        output_id: Option<&str>,
        output_sr: u32,
        block_frames: usize,
        valid: &ValidGraph,
        input_sr: u32,
        realtime: bool,
    ) -> (BuiltOutputGraph, HashMap<String, Producer<f32>>) {
        let mut producer_pairs = Vec::new();
        let native = valid
            .inputs
            .iter()
            .map(|i| (i.id.clone(), input_sr))
            .collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let mut reg = fresh_registry();
        let built = build_output_graph(
            output_id,
            output_sr,
            block_frames,
            realtime,
            valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            HashMap::new(),
            &HashMap::new(),
        )
        .expect("build succeeds");
        let mut map = HashMap::new();
        for (id, prod) in producer_pairs {
            map.insert(id, prod);
        }
        (built, map)
    }

    #[test]
    fn passthrough_speaker_reproduces_source_after_two_blocks() {
        let (valid, _) = passthrough_graph();
        let (mut built, mut producers) = build(Some("s"), SR, &valid, SR, false);
        assert_eq!(built.graph.sample_rate(), SR);
        assert_eq!(built.graph.out_channels(), 2);
        assert_eq!(built.graph.latency_frames(), 0);

        // Feed 4 blocks; the first block drains as the ring fills, the rest
        // must be an exact copy of the source ramp.
        let mut fed = Vec::new();
        for k in 0..4 {
            fed.extend(stereo_ramp(1024, k as f32 * 1024.0));
        }
        let pushed = push_all(producers.get_mut("m").unwrap(), &fed);
        assert_eq!(pushed, fed.len());

        let mut out1 = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out1);
        let mut out2 = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out2);
        assert_eq!(
            built
                .output
                .blocks
                .load(std::sync::atomic::Ordering::Relaxed),
            2
        );

        // With zero effect latency and equal rates the ramp streams straight
        // through: block 1 is its first 1024 frames, block 2 the next span.
        assert_eq!(out1, fed[..TIMER_BLOCK_FRAMES * 2]);
        assert_eq!(out2, fed[TIMER_BLOCK_FRAMES * 2..TIMER_BLOCK_FRAMES * 4]);
    }

    #[test]
    fn volume_atom_scales_output() {
        let (valid, _) = passthrough_graph();
        let mut volumes: HashMap<String, Arc<AtomicU32>> = HashMap::new();
        volumes.insert("m".to_string(), Arc::new(AtomicU32::new(1.0f32.to_bits())));
        let mut producer_pairs = Vec::new();
        let native = valid.inputs.iter().map(|i| (i.id.clone(), SR)).collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let mut reg = fresh_registry();
        let mut built = build_output_graph(
            Some("s"),
            SR,
            TIMER_BLOCK_FRAMES,
            false,
            &valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &volumes,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            HashMap::new(),
            &HashMap::new(),
        )
        .expect("build");
        let prod = &mut producer_pairs[0].1;
        let fed = stereo_ramp(4096, 0.0);
        push_all(prod, &fed);

        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out);
        built.graph.process_block(&mut out);
        built.graph.process_block(&mut out);

        // Drop the volume to 0.5 and confirm the next block scales.
        volumes["m"].store(0.5f32.to_bits(), std::sync::atomic::Ordering::Relaxed);
        built.graph.process_block(&mut out);
        let want = &fed[TIMER_BLOCK_FRAMES * 6..TIMER_BLOCK_FRAMES * 8];
        for (o, w) in out.iter().zip(want) {
            assert!((o - 0.5 * w).abs() < 1e-5, "{o} vs {w}");
        }
    }

    #[test]
    fn paused_source_drains_ring_and_silences() {
        let (valid, _) = passthrough_graph();
        let mut paused: HashMap<String, Arc<AtomicBool>> = HashMap::new();
        paused.insert("m".to_string(), Arc::new(AtomicBool::new(true)));
        let mut producer_pairs = Vec::new();
        let native = valid.inputs.iter().map(|i| (i.id.clone(), SR)).collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let mut reg = fresh_registry();
        let mut built = build_output_graph(
            Some("s"),
            SR,
            TIMER_BLOCK_FRAMES,
            false,
            &valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &HashMap::new(),
            &paused,
            &HashMap::new(),
            &HashMap::new(),
            HashMap::new(),
            &HashMap::new(),
        )
        .expect("build");
        let prod = &mut producer_pairs[0].1;
        push_all(prod, &stereo_ramp(4096, 0.0));
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out);
        assert_eq!(out, vec![0.0; TIMER_BLOCK_FRAMES * 2]);
        // The ring was drained: no backlog left.
        let meta = &built.sources[0];
        assert_eq!(
            meta.stats.level.load(std::sync::atomic::Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn drain_generation_change_flushes_the_source() {
        let (valid, _) = passthrough_graph();
        let mut drain: HashMap<String, Arc<AtomicU64>> = HashMap::new();
        drain.insert("m".to_string(), Arc::new(AtomicU64::new(0)));
        let mut producer_pairs = Vec::new();
        let native = valid.inputs.iter().map(|i| (i.id.clone(), SR)).collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let mut reg = fresh_registry();
        let mut built = build_output_graph(
            Some("s"),
            SR,
            TIMER_BLOCK_FRAMES,
            false,
            &valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &drain,
            &HashMap::new(),
            HashMap::new(),
            &HashMap::new(),
        )
        .expect("build");
        let prod = &mut producer_pairs[0].1;
        push_all(prod, &stereo_ramp(4096, 0.0));

        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out);
        built.graph.process_block(&mut out);
        // A seek rewinds the capture: generation bumps, everything is dropped.
        drain["m"].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        built.graph.process_block(&mut out);
        assert_eq!(out, vec![0.0; TIMER_BLOCK_FRAMES * 2]);
        // The flushed ring stays empty: the next block is silence too.
        let mut out2 = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out2);
        assert_eq!(out2, vec![0.0; TIMER_BLOCK_FRAMES * 2]);
    }

    /// Mic into a plain gain and a limiter (2 ms lookahead) in parallel: a
    /// source ring, an effect that holds audio, and a delay line padding the
    /// plain branch. The branches meet at the speaker, or with `mix` at a gain
    /// before it, so the delay line sits on a terminal or an effect's input.
    /// `meter` adds a level meter off the mic, an edit that touches none of it.
    pub(in crate::audio::pipeline) fn parallel_with_lookahead(
        meter: bool,
        lookahead_ms: f32,
        mix: bool,
    ) -> ValidGraph {
        let mut nodes = vec![
            mic("m"),
            gain_node("g", 0.0),
            gain_node("l", 0.0),
            speaker("s"),
        ];
        let meet = if mix { "x" } else { "s" };
        let mut edges = vec![
            edge("e1", "m", None, "g", None),
            edge("e2", "m", None, "l", None),
            edge("e3", "g", None, meet, None),
            edge("e4", "l", None, meet, None),
        ];
        if mix {
            nodes.push(gain_node("x", 0.0));
            edges.push(edge("e6", "x", None, "s", None));
        }
        if meter {
            nodes.push(node("lm", NodeKind::LevelMeter, serde_json::json!({})));
            edges.push(edge("e5", "m", None, "lm", None));
        }
        let mut valid = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes,
            edges,
        }
        .validate()
        .expect("valid");
        for e in &mut valid.effects {
            if e.id == "l" {
                e.spec = EffectSpec::Limiter(crate::audio::graph::LimiterData {
                    ceiling_db: 0.0,
                    lookahead_ms,
                    release_ms: 50.0,
                    bypassed: false,
                });
            }
        }
        valid
    }

    /// Builds `valid` at 64-frame blocks with `registry` kept across builds,
    /// as the pipeline keeps it, carrying over from `previous`.
    pub(in crate::audio::pipeline) fn rebuild(
        valid: &ValidGraph,
        registry: &mut EffectRegistry,
        previous: &HashMap<String, CarriedNode>,
        realtime: bool,
    ) -> (BuiltOutputGraph, HashMap<String, Producer<f32>>) {
        let mut pairs = Vec::new();
        let native = valid.inputs.iter().map(|i| (i.id.clone(), SR)).collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let built = build_output_graph(
            Some("s"),
            SR,
            64,
            realtime,
            valid,
            &native,
            &native_ch,
            &mut pairs,
            registry,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            HashMap::new(),
            previous,
        )
        .expect("build succeeds");
        (built, pairs.into_iter().collect())
    }

    #[test]
    fn an_edit_elsewhere_leaves_the_running_audio_untouched() {
        for mix in [false, true] {
            edit_elsewhere(mix);
        }
    }

    fn edit_elsewhere(mix: bool) {
        // The limiter's lookahead and the plain branch's delay line both hold
        // 96 frames. Rebuilt from scratch they would restart from silence and
        // the source from an empty ring; carried over, the mix goes on sample
        // for sample as if no swap happened.
        let mut registry = fresh_registry();
        let (mut a, mut producers) = rebuild(
            &parallel_with_lookahead(false, 2.0, mix),
            &mut registry,
            &HashMap::new(),
            false,
        );
        let input = producers.get_mut("m").unwrap();
        let fed = quiet_ramp(64 * 40);
        push_all(input, &fed);
        let mut got = render(&mut a.graph, 20);

        let (mut b, fresh) = rebuild(
            &parallel_with_lookahead(true, 2.0, mix),
            &mut registry,
            &a.carried,
            false,
        );
        assert!(
            !fresh.contains_key("m"),
            "a carried source gets no new ring"
        );
        assert_eq!(b.carried_inputs, vec!["m".to_string()]);
        assert_eq!(b.graph.latency_frames(), 96);
        assert!(b.graph.adopt_from(&mut a.graph), "nodes carried over");
        got.extend(render(&mut b.graph, 20));

        let pad = 96;
        for f in pad..64 * 40 {
            let want = 2.0 * fed[(f - pad) * 2];
            assert!(
                (got[f * 2] - want).abs() < 1e-5,
                "mix {mix}, frame {f}: got {} want {want}",
                got[f * 2]
            );
        }
    }

    #[test]
    fn a_structural_change_rebuilds_only_that_node() {
        let mut registry = fresh_registry();
        let (a, _) = rebuild(
            &parallel_with_lookahead(false, 2.0, false),
            &mut registry,
            &HashMap::new(),
            false,
        );
        let (b, _) = rebuild(
            &parallel_with_lookahead(false, 4.0, false),
            &mut registry,
            &a.carried,
            false,
        );
        let effect = |g: &OutputGraph, id: &str| {
            let i = g.node_ids.iter().position(|n| n == id).expect("node");
            match &g.nodes[i] {
                DagNode::Effect(e) => e.effects.len(),
                _ => panic!("{id} is not an effect"),
            }
        };
        assert_eq!(
            effect(&b.graph, "g"),
            0,
            "the untouched gain is carried over"
        );
        assert!(
            effect(&b.graph, "l") > 0,
            "the changed limiter is built anew"
        );
        assert_eq!(b.graph.latency_frames(), 192, "4 ms lookahead @ 48k");
    }

    #[test]
    fn a_live_source_keeps_playing_across_a_swap() {
        // A locked live source, fed a block per block: once playing, a swap
        // must not re-prime it (silence) or realign it (a jump).
        let mut registry = fresh_registry();
        let valid_a = passthrough_graph().0;
        let valid_b = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                gain_node("g", 0.0),
                speaker("s"),
                node("lm", NodeKind::LevelMeter, serde_json::json!({})),
            ],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "g", None, "s", None),
                edge("e3", "m", None, "lm", None),
            ],
        }
        .validate()
        .expect("valid");
        let (mut a, mut producers) = rebuild(&valid_a, &mut registry, &HashMap::new(), true);
        a.graph.lock_inputs(&HashSet::from(["m".to_string()]));
        let mut input = producers.remove("m").unwrap();
        let mut next = 1usize;
        let mut feed_block = |input: &mut Producer<f32>| {
            let block: Vec<f32> = (0..64)
                .flat_map(|i| {
                    let v = (next + i) as f32;
                    [v, v]
                })
                .collect();
            next += 64;
            push_all(input, &block);
        };
        for _ in 0..8 {
            feed_block(&mut input);
        }
        let mut out = vec![0.0; 64 * 2];
        let mut played = Vec::new();
        for _ in 0..200 {
            feed_block(&mut input);
            a.graph.process_block(&mut out);
            played.extend(out.iter().step_by(2).copied());
        }
        let (mut b, _) = rebuild(&valid_b, &mut registry, &a.carried, true);
        b.graph.lock_inputs(&HashSet::from(["m".to_string()]));
        assert!(b.graph.adopt_from(&mut a.graph));
        for _ in 0..200 {
            feed_block(&mut input);
            b.graph.process_block(&mut out);
            played.extend(out.iter().step_by(2).copied());
        }
        let tail = &played[100 * 64..];
        for w in tail.windows(2) {
            assert_eq!(w[1], w[0] + 1.0, "the source skipped or repeated audio");
        }
    }

    /// `passthrough_graph` plus a level meter off the mic: an edit that
    /// leaves the audible chain as it was.
    pub(in crate::audio::pipeline) fn passthrough_with_meter() -> ValidGraph {
        GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                gain_node("g", 0.0),
                speaker("s"),
                node("lm", NodeKind::LevelMeter, serde_json::json!({})),
            ],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "g", None, "s", None),
                edge("e3", "m", None, "lm", None),
            ],
        }
        .validate()
        .expect("valid")
    }

    fn worst_step(samples: &[f32]) -> f32 {
        samples
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max)
    }

    #[test]
    fn inserting_an_effect_on_a_carried_source_does_not_step() {
        // The mic is carried over, so the swap does not crossfade; the gain
        // put in its path must still come in without a step.
        let mut registry = fresh_registry();
        let valid_b = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                gain_node("g", 0.0),
                gain_node("g2", -20.0),
                speaker("s"),
            ],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "g", None, "g2", None),
                edge("e3", "g2", None, "s", None),
            ],
        }
        .validate()
        .expect("valid");
        let (mut a, mut producers) = rebuild(
            &passthrough_graph().0,
            &mut registry,
            &HashMap::new(),
            false,
        );
        push_all(producers.get_mut("m").unwrap(), &vec![0.5; 64 * 2 * 60]);
        let (b, _) = rebuild(&valid_b, &mut registry, &a.carried, false);
        assert!(
            !b.graph.plays_like(&a.graph),
            "the gain changes what is heard"
        );
        let mut played = Vec::new();
        played.extend(render(&mut a.graph, 10).into_iter().step_by(2));
        let (mut worker, mut ctrl) = super::super::worker::dsp_worker(a.graph);
        ctrl.send_graph(b.graph).expect("swap");
        let mut out = vec![0.0; 64 * 2];
        for _ in 0..30 {
            worker.next_block(&mut out);
            played.extend(out.iter().step_by(2));
        }
        assert!(
            (played[played.len() - 1] - 0.05).abs() < 1e-4,
            "the new gain plays"
        );
        let worst = worst_step(&played[64 * 4..]);
        let fade = (SR as usize * 10 / 1000) as f32;
        assert!(worst <= 0.5 / fade + 1e-4, "a step of {worst}");
    }

    #[test]
    fn a_carried_source_follows_the_new_graphs_clock_lock() {
        // The tap was on the speaker's clock; the default output moved, so the
        // new graph reads it through the drift resampler. Carrying the source
        // over must not bring the old lock with it.
        let mut registry = fresh_registry();
        let (mut a, _p) = rebuild(&passthrough_graph().0, &mut registry, &HashMap::new(), true);
        a.graph.lock_inputs(&HashSet::from(["m".to_string()]));
        let (mut b, _) = rebuild(&passthrough_with_meter(), &mut registry, &a.carried, true);
        b.graph.lock_inputs(&HashSet::new());
        assert!(b.graph.adopt_from(&mut a.graph), "the mic is carried over");
        let steered = b
            .graph
            .nodes
            .iter()
            .any(|n| matches!(n, DagNode::Source(s) if s.asrc.is_some()));
        assert!(steered, "the carried source still reads locked");
    }

    #[test]
    fn live_updates_reach_a_carried_effect() {
        // A carried gain keeps the instance the pipeline's control drives.
        let mut registry = fresh_registry();
        let (mut a, mut producers) = rebuild(
            &passthrough_graph().0,
            &mut registry,
            &HashMap::new(),
            false,
        );
        let control = a
            .controls
            .iter()
            .find(|(id, _)| id == "g")
            .map(|(_, c)| c.clone())
            .expect("gain control");
        let bypass = a
            .bypasses
            .iter()
            .find(|(id, _)| id == "g")
            .map(|(_, b)| b.clone())
            .expect("gain bypass");
        push_all(producers.get_mut("m").unwrap(), &vec![0.5; 64 * 2 * 60]);
        render(&mut a.graph, 4);
        let (mut b, _) = rebuild(&passthrough_with_meter(), &mut registry, &a.carried, false);
        assert!(b.graph.adopt_from(&mut a.graph));

        control.apply_update(&serde_json::json!({ "gainDb": -6.0 }));
        let got = render(&mut b.graph, 20);
        let want = 0.5 * 10f32.powf(-6.0 / 20.0);
        assert!(
            (got[got.len() - 1] - want).abs() < 1e-3,
            "gain update: got {} want {want}",
            got[got.len() - 1]
        );

        bypass.store(true, Ordering::Relaxed);
        let got = render(&mut b.graph, 4);
        assert!(
            (got[got.len() - 1] - 0.5).abs() < 1e-6,
            "bypass: got {}",
            got[got.len() - 1]
        );
    }

    #[test]
    fn a_chain_of_carried_swaps_keeps_every_node() {
        // Two edits queued before the worker takes either: the second graph
        // was built against the first, which was built against the running
        // one, and the worker adopts them in order in one drain.
        let mut registry = fresh_registry();
        let (mut a, mut producers) = rebuild(
            &parallel_with_lookahead(false, 2.0, false),
            &mut registry,
            &HashMap::new(),
            false,
        );
        let fed = quiet_ramp(64 * 60);
        push_all(producers.get_mut("m").unwrap(), &fed);
        let mut got = render(&mut a.graph, 20);
        let (mut b, _) = rebuild(
            &parallel_with_lookahead(true, 2.0, false),
            &mut registry,
            &a.carried,
            false,
        );
        let (mut c, _) = rebuild(
            &parallel_with_lookahead(false, 2.0, false),
            &mut registry,
            &b.carried,
            false,
        );
        assert!(b.graph.adopt_from(&mut a.graph));
        assert!(c.graph.adopt_from(&mut b.graph));
        for n in &c.graph.nodes {
            if let DagNode::Effect(e) = n {
                assert!(!e.effects.is_empty(), "a placeholder was never filled");
            }
        }
        got.extend(render(&mut c.graph, 20));
        let pad = 96;
        for f in pad..64 * 40 {
            let want = 2.0 * fed[(f - pad) * 2];
            assert!(
                (got[f * 2] - want).abs() < 1e-5,
                "frame {f}: got {} want {want}",
                got[f * 2]
            );
        }
    }

    #[test]
    fn a_resampled_source_that_goes_quiet_keeps_its_queue() {
        // A 44.1 kHz capture read into a 48 kHz graph: the judge counts time
        // in the capture's frames, which a block covers only approximately.
        // Pauses in its delivery must still not deepen its queue.
        const IN_SR: u32 = 44_100;
        const BURST: usize = 441;
        let worst_queue = |gaps: &[(f64, f64)]| -> u64 {
            let (valid, _) = passthrough_graph();
            let (mut built, mut producers) =
                build_with_block(Some("s"), SR, 64, &valid, IN_SR, true);
            let clock = Arc::new(WriteClock::default());
            built.graph.attach_write_clock("m", &clock);
            let stats = built.sources[0].stats.clone();
            let input = producers.get_mut("m").unwrap();
            let out_period = 64.0 / SR as f64;
            let in_period = BURST as f64 / IN_SR as f64;
            let (mut t_in, mut t_out) = (in_period * 0.37, 0.0);
            let chunk = vec![0.25_f32; BURST * 2];
            let mut out = vec![0.0; 64 * 2];
            let mut worst = 0;
            while t_out < 60.0 {
                if t_in <= t_out {
                    if !gaps.iter().any(|&(a, b)| t_in >= a && t_in < b) {
                        clock.record(chunk.len(), t_in);
                        push_all(input, &chunk);
                    }
                    t_in += in_period;
                    continue;
                }
                built.graph.process_block_at(&mut out, t_out);
                if t_out >= 5.0 {
                    worst = worst.max(stats.queue_frames.load(Ordering::Relaxed));
                }
                t_out += out_period;
            }
            worst
        };
        let gaps: Vec<(f64, f64)> = (0..100)
            .map(|k| {
                let start = 5.0 + k as f64 * 0.5 + 0.2;
                (start, start + 0.06 + (k % 7) as f64 * 0.05)
            })
            .collect();
        let steady = worst_queue(&[]);
        let gappy = worst_queue(&gaps);
        assert!(
            gappy <= steady + BURST as u64,
            "silence deepened the queue: {gappy} frames vs {steady} steady"
        );
    }

    /// Mic → flat EQ → Mute → 0 dB gain → speaker, with `meter` adding a
    /// level meter off the mute: the chain from the field report.
    fn mute_chain(meter: bool) -> ValidGraph {
        let mut nodes = vec![
            mic("m"),
            node(
                "e",
                NodeKind::Eq,
                serde_json::json!({ "gainsDb": vec![0.0; 10] }),
            ),
            node("mu", NodeKind::Mute, serde_json::json!({ "muted": false })),
            gain_node("g", 0.0),
            speaker("s"),
        ];
        let mut edges = vec![
            edge("e1", "m", None, "e", None),
            edge("e2", "e", None, "mu", None),
            edge("e3", "mu", None, "g", None),
            edge("e4", "g", None, "s", None),
        ];
        if meter {
            nodes.push(node("lm", NodeKind::LevelMeter, serde_json::json!({})));
            edges.push(edge("e5", "mu", None, "lm", None));
        }
        GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes,
            edges,
        }
        .validate()
        .expect("valid")
    }

    #[test]
    fn a_chain_through_mute_stays_at_unity() {
        // Through a rebuild that carries the chain over, and the mute switched
        // off and on again live, a 0.5 sine never comes out above 0.5.
        for width in [2usize, 16] {
            let mut registry = fresh_registry();
            let (mut a, mut producers) =
                rebuild(&mute_chain(false), &mut registry, &HashMap::new(), false);
            a.graph.set_out_channels(width);
            let muted = a
                .controls
                .iter()
                .find_map(|(id, c)| match c {
                    EffectControl::Mute { muted } if id == "mu" => Some(muted.clone()),
                    _ => None,
                })
                .expect("mute control");
            let input = producers.get_mut("m").unwrap();
            let sine: Vec<f32> = (0..48_000)
                .flat_map(|n| {
                    let v = ((n % 480) as f32 / 480.0 * std::f32::consts::TAU).sin() * 0.5;
                    [v, v]
                })
                .collect();
            push_all(input, &sine);
            let mut out = vec![0.0; 64 * width];
            let mut peak = 0.0f32;
            for _ in 0..200 {
                a.graph.process_block(&mut out);
                peak = out.iter().fold(peak, |m, s| m.max(s.abs()));
            }
            let (mut b, _) = rebuild(&mute_chain(true), &mut registry, &a.carried, false);
            b.graph.set_out_channels(width);
            assert!(b.graph.adopt_from(&mut a.graph));
            for k in 0..500 {
                if k == 100 {
                    muted.store(true, Ordering::Relaxed);
                }
                if k == 200 {
                    muted.store(false, Ordering::Relaxed);
                }
                b.graph.process_block(&mut out);
                peak = out.iter().fold(peak, |m, s| m.max(s.abs()));
            }
            assert!(peak <= 0.5 + 1e-5, "{width} channels: peak {peak}");
        }
    }

    #[test]
    fn full_volume_is_unity_gain_into_a_wide_device() {
        // A stereo source at 100% into a 16-channel device through a 0 dB
        // gain: its two channels come out at exactly the level they went in,
        // and nothing else is fed.
        let (valid, _) = passthrough_graph();
        let (mut built, mut producers) = build_with_block(Some("s"), SR, 64, &valid, SR, false);
        built.graph.set_out_channels(16);
        push_all(producers.get_mut("m").unwrap(), &vec![0.5; 64 * 2 * 8]);
        let mut out = vec![0.0; 64 * 16];
        for _ in 0..4 {
            built.graph.process_block(&mut out);
        }
        for frame in out.chunks_exact(16) {
            assert_eq!(&frame[..2], &[0.5, 0.5]);
            assert!(frame[2..].iter().all(|&s| s == 0.0));
        }
    }

    #[test]
    fn the_monitor_sources_are_keyed_like_its_bridges() {
        // Delivery counters are joined to a source by (input, output); the
        // monitor's bridges are keyed by `MONITOR_KEY`, so its sources must be
        // too, or a paused app reads as a pipeline falling behind.
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                node("lm", NodeKind::LevelMeter, serde_json::json!({})),
            ],
            edges: vec![edge("e1", "m", None, "lm", None)],
        };
        let valid = g.validate().expect("valid");
        let (built, _) = build(None, SR, &valid, SR, true);
        assert_eq!(built.sources[0].output_id, MONITOR_KEY);
    }

    #[test]
    fn limiter_latency_pads_the_parallel_path() {
        // Two branches to one output: a passthrough gain and a limiter with
        // lookahead. The graph must report the lookahead as its latency.
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                gain_node("g", 0.0),
                gain_node("l", 0.0),
                speaker("s"),
            ],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "m", None, "l", None),
                edge("e3", "g", None, "s", None),
                edge("e4", "l", None, "s", None),
            ],
        };
        // Replace one gain with a limiter spec by editing its node kind.
        let mut valid = g.validate().expect("valid");
        for e in &mut valid.effects {
            if e.id == "l" {
                e.spec = EffectSpec::Limiter(crate::audio::graph::LimiterData {
                    ceiling_db: 0.0,
                    lookahead_ms: 2.0,
                    release_ms: 50.0,
                    bypassed: false,
                });
            }
        }
        let (built, _) = build(Some("s"), SR, &valid, SR, false);
        assert_eq!(built.graph.latency_frames(), 96, "2 ms lookahead @ 48k");
        // Both paths are padded to the same length → mix stays aligned.
        assert_eq!(built.graph.active_output_channels(), 2);
    }

    fn quiet_ramp(frames: usize) -> Vec<f32> {
        (0..frames * 2)
            .map(|i| ((i / 2) % 997) as f32 * 1e-4)
            .collect()
    }

    fn render(graph: &mut OutputGraph, blocks: usize) -> Vec<f32> {
        let mut got = Vec::new();
        let mut out = vec![0.0; graph.block_frames() * 2];
        for _ in 0..blocks {
            graph.process_block(&mut out);
            got.extend_from_slice(&out);
        }
        got
    }

    #[test]
    fn every_buffer_size_streams_the_source_sample_exact() {
        let (valid, _) = passthrough_graph();
        for block in crate::audio::graph::BUFFER_FRAME_OPTIONS.map(|n| n as usize) {
            let (mut built, mut producers) =
                build_with_block(Some("s"), SR, block, &valid, SR, false);
            assert_eq!(built.graph.block_frames(), block);
            let fed = stereo_ramp(8192, 0.0);
            push_all(producers.get_mut("m").unwrap(), &fed);
            let blocks = 8192 / block;
            let got = render(&mut built.graph, blocks);
            assert_eq!(got.len(), blocks * block * 2);
            assert_eq!(got, fed[..got.len()], "{block}-frame blocks");
        }
    }

    #[test]
    fn delay_compensation_spans_several_small_blocks() {
        // Limiter lookahead (96 frames) is three 32-frame blocks: the dry branch
        // must be held back across block boundaries, not just within one.
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                gain_node("g", 0.0),
                gain_node("l", 0.0),
                speaker("s"),
            ],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "m", None, "l", None),
                edge("e3", "g", None, "s", None),
                edge("e4", "l", None, "s", None),
            ],
        };
        let mut valid = g.validate().expect("valid");
        for e in &mut valid.effects {
            if e.id == "l" {
                e.spec = EffectSpec::Limiter(crate::audio::graph::LimiterData {
                    ceiling_db: 0.0,
                    lookahead_ms: 2.0,
                    release_ms: 50.0,
                    bypassed: false,
                });
            }
        }
        let (mut built, mut producers) = build_with_block(Some("s"), SR, 32, &valid, SR, false);
        let pad = built.graph.latency_frames();
        assert_eq!(pad, 96);
        let fed = quiet_ramp(4096);
        push_all(producers.get_mut("m").unwrap(), &fed);
        let got = render(&mut built.graph, 4096 / 32);
        for f in pad..4096 {
            let want = 2.0 * fed[(f - pad) * 2];
            assert!(
                (got[f * 2] - want).abs() < 1e-5,
                "frame {f}: got {} want {want}",
                got[f * 2]
            );
        }
    }

    /// Drives a live passthrough source the way a capture device and an output
    /// device do, each on its own clock: `burst`-frame deliveries against
    /// `block`-frame reads, the source read through the drift resampler with
    /// the capture's write timing, as the pipeline wires it. Returns the
    /// output after `settle` seconds and the frames spliced out after it.
    fn live_capture(
        block: usize,
        burst: usize,
        capture_ppm: f64,
        seconds: f64,
        settle: f64,
        signal: impl Fn(usize) -> f32,
    ) -> (Vec<f32>, SourceStats, u64) {
        let (valid, _) = passthrough_graph();
        let (mut built, mut producers) = build_with_block(Some("s"), SR, block, &valid, SR, true);
        let clock = Arc::new(WriteClock::default());
        built.graph.attach_write_clock("m", &clock);
        let stats = built.sources[0].stats.clone();
        let input = producers.get_mut("m").unwrap();
        let out_period = block as f64 / SR as f64;
        let in_period = burst as f64 / (SR as f64 * (1.0 + capture_ppm * 1e-6));
        let (mut t_in, mut t_out, mut fed) = (in_period * 0.37, 0.0, 0usize);
        let mut out = vec![0.0; block * 2];
        let mut kept = Vec::new();
        let mut chunk = vec![0.0; burst * 2];
        let mut trimmed_at_settle = 0;
        while t_out < seconds {
            if t_in <= t_out {
                for f in 0..burst {
                    let v = signal(fed + f);
                    chunk[f * 2] = v;
                    chunk[f * 2 + 1] = v;
                }
                clock.record(chunk.len(), t_in);
                assert_eq!(push_all(input, &chunk), chunk.len());
                fed += burst;
                t_in += in_period;
                continue;
            }
            if t_out < settle {
                trimmed_at_settle = stats.trimmed.load(Ordering::Relaxed);
            }
            built.graph.process_block_at(&mut out, t_out);
            if t_out >= settle {
                kept.extend(out.iter().step_by(2));
            }
            t_out += out_period;
        }
        let spliced = stats.trimmed.load(Ordering::Relaxed) - trimmed_at_settle;
        (kept, stats, spliced / 2)
    }

    /// One observation per output block of a live source whose capture goes
    /// quiet during `gaps` (a paused app delivers nothing at all).
    struct Tick {
        t: f64,
        /// Frames between the newest captured frame and the one just played;
        /// `None` while the output is silent.
        real_delay: Option<usize>,
        reported_queue: usize,
    }

    /// `locked`: the capture runs on the output's clock and every played
    /// sample names its frame. Otherwise it is read through the drift
    /// resampler, whose samples are interpolated, and the delay is what it has
    /// taken from the ring.
    fn live_capture_with_gaps(
        block: usize,
        burst: usize,
        seconds: f64,
        gaps: &[(f64, f64)],
        locked: bool,
    ) -> Vec<Tick> {
        let (valid, _) = passthrough_graph();
        let (mut built, mut producers) = build_with_block(Some("s"), SR, block, &valid, SR, true);
        let clock = Arc::new(WriteClock::default());
        if locked {
            built.graph.lock_inputs(&HashSet::from(["m".to_string()]));
        } else {
            built.graph.attach_write_clock("m", &clock);
        }
        let stats = built.sources[0].stats.clone();
        let input = producers.get_mut("m").unwrap();
        let out_period = block as f64 / SR as f64;
        let in_period = burst as f64 / SR as f64;
        let (mut t_in, mut t_out, mut fed) = (in_period * 0.37, 0.0, 0usize);
        let mut out = vec![0.0; block * 2];
        let mut chunk = vec![0.0; burst * 2];
        let mut ticks = Vec::new();
        while t_out < seconds {
            if t_in <= t_out {
                let paused = gaps.iter().any(|&(a, b)| t_in >= a && t_in < b);
                if !paused {
                    // Each sample carries its own capture index (+1, so 0 is silence).
                    for f in 0..burst {
                        let v = (fed + f + 1) as f32;
                        chunk[f * 2] = v;
                        chunk[f * 2 + 1] = v;
                    }
                    clock.record(chunk.len(), t_in);
                    push_all(input, &chunk);
                    fed += burst;
                }
                t_in += in_period;
                continue;
            }
            built.graph.process_block_at(&mut out, t_out);
            let last = out[(block - 1) * 2];
            let consumed = stats.consumed.load(Ordering::Relaxed) as usize / 2;
            let played = if locked { last as usize } else { consumed };
            ticks.push(Tick {
                t: t_out,
                real_delay: (last > 0.0).then(|| fed.saturating_sub(played)),
                reported_queue: stats.queue_frames.load(Ordering::Relaxed) as usize,
            });
            t_out += out_period;
        }
        ticks
    }

    #[test]
    fn a_paused_app_resumes_at_low_latency_every_time() {
        let gaps = [(10.0, 15.0), (25.0, 30.0), (40.0, 41.0)];
        for locked in [true, false] {
            resumes_at_low_latency(&live_capture_with_gaps(64, 480, 55.0, &gaps, locked));
        }
    }

    fn resumes_at_low_latency(ticks: &[Tick]) {
        for (resume, until) in [(15.0, 25.0), (30.0, 40.0), (41.0, 55.0)] {
            let window: Vec<&Tick> = ticks
                .iter()
                .filter(|k| k.t >= resume + 1.0 && k.t < until)
                .collect();
            let playing = window.iter().filter(|k| k.real_delay.is_some()).count();
            assert!(
                playing * 10 >= window.len() * 9,
                "after resuming at {resume} s only {playing}/{} blocks played",
                window.len()
            );
            let worst = window
                .iter()
                .filter_map(|k| k.real_delay)
                .max()
                .unwrap_or(0);
            assert!(
                worst < 480 + 4 * 64 + 480,
                "resumed at {resume} s with {worst} frames of delay"
            );
        }
    }

    fn worst_delay_between(ticks: &[Tick], from: f64, to: f64) -> usize {
        ticks
            .iter()
            .filter(|k| k.t >= from && k.t < to)
            .filter_map(|k| k.real_delay)
            .max()
            .unwrap_or(0)
    }

    fn worst_delay(ticks: &[Tick], from: f64) -> usize {
        ticks
            .iter()
            .filter(|k| k.t >= from)
            .filter_map(|k| k.real_delay)
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn silence_between_sounds_never_becomes_latency() {
        // An app playing 200 ms sounds with 60-360 ms of nothing between them,
        // for two minutes: the delay must stay what a steady source gets.
        let gaps: Vec<(f64, f64)> = (0..240)
            .map(|k| {
                let start = 5.0 + k as f64 * 0.5 + 0.2;
                (start, start + 0.06 + (k % 7) as f64 * 0.05)
            })
            .collect();
        for locked in [true, false] {
            silence_adds_no_latency(&gaps, locked);
        }
    }

    fn silence_adds_no_latency(gaps: &[(f64, f64)], locked: bool) {
        let steady = live_capture_with_gaps(64, 480, 125.0, &[], locked);
        let gappy = live_capture_with_gaps(64, 480, 125.0, gaps, locked);
        let baseline = worst_delay(&steady, 3.0);
        let with_gaps = worst_delay(&gappy, 3.0);
        // A sound resuming after silence may start up to one delivery above
        // target (its onset is played, not cut); that is the whole allowance.
        assert!(
            with_gaps <= baseline + 480,
            "locked {locked}: silence added latency: {with_gaps} frames vs {baseline} steady"
        );
        // And it never accumulates: after a hundred pauses it is where the
        // first few left it.
        let early = worst_delay_between(&gappy, 6.0, 20.0);
        let late = worst_delay_between(&gappy, 100.0, 125.0);
        assert!(
            late <= early + 64,
            "locked {locked}: latency crept across pauses: {early} -> {late}"
        );
    }

    #[test]
    fn readout_does_not_climb_across_pauses() {
        let gaps: Vec<(f64, f64)> = (0..40).map(|k| (5.0 + k as f64, 5.4 + k as f64)).collect();
        let ticks = live_capture_with_gaps(64, 480, 45.0, &gaps, false);
        // Measured once the pauses have begun, so both sides include a resume.
        let early = ticks
            .iter()
            .filter(|k| (5.0..10.0).contains(&k.t))
            .map(|k| k.reported_queue)
            .max()
            .unwrap();
        let late = ticks
            .iter()
            .filter(|k| k.t > 40.0)
            .map(|k| k.reported_queue)
            .max()
            .unwrap();
        assert!(
            late <= early + 64,
            "readout crept from {early} to {late} frames"
        );
    }

    struct TapRun {
        /// Captured frames never played: cut out by splices or discards.
        skipped: usize,
        /// Largest stretch of audio lost in one place. A splice removes at most
        /// a few dozen frames under a crossfade; anything bigger is an audible
        /// jump.
        worst_jump: usize,
        /// Largest stretch lost while audio was playing, rather than under a
        /// dropout's silence.
        worst_jump_heard: usize,
        /// Output samples of silence once playing, not counting pauses.
        dropouts: usize,
        worst_delay: usize,
    }

    /// A process-tap-like capture: `burst`-frame deliveries, each up to
    /// `jitter_ms` late, plus `stalls` where deliveries are held back and then
    /// arrive all at once. Every captured frame is numbered, so the output
    /// shows exactly which frames were skipped.
    fn tap_capture(
        block: usize,
        burst: usize,
        seconds: f64,
        settle: f64,
        jitter_ms: f64,
        stalls: &[(f64, f64)],
    ) -> TapRun {
        let (valid, _) = passthrough_graph();
        let (mut built, mut producers) = build_with_block(Some("s"), SR, block, &valid, SR, true);
        built.graph.lock_inputs(&HashSet::from(["m".to_string()]));
        let input = producers.get_mut("m").unwrap();
        let out_period = block as f64 / SR as f64;
        let in_period = burst as f64 / SR as f64;
        let (mut k, mut t_out, mut fed) = (0usize, 0.0, 0usize);
        let mut seed: u32 = 0x9e37_79b9;
        let mut due = in_period;
        let mut out = vec![0.0; block * 2];
        let mut chunk = vec![0.0; burst * 2];
        let mut run = TapRun {
            skipped: 0,
            worst_jump: 0,
            worst_jump_heard: 0,
            dropouts: 0,
            worst_delay: 0,
        };
        let mut last_played = 0usize;
        let mut prev_raw = 0.0f32;
        let mut silent_since = false;
        while t_out < seconds {
            if due <= t_out {
                for f in 0..burst {
                    let v = (fed + f + 1) as f32;
                    chunk[f * 2] = v;
                    chunk[f * 2 + 1] = v;
                }
                push_all(input, &chunk);
                fed += burst;
                k += 1;
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let nominal = (k + 1) as f64 * in_period;
                let jitter = jitter_ms / 1000.0 * (seed % 1000) as f64 / 1000.0;
                // Inside a stall a delivery waits for the stall to clear.
                due = stalls
                    .iter()
                    .find(|&&(a, b)| nominal >= a && nominal < b)
                    .map_or(nominal + jitter, |&(_, b)| b);
                continue;
            }
            built.graph.process_block(&mut out);
            if t_out >= settle {
                for f in 0..block {
                    let raw = out[f * 2];
                    if raw == 0.0 {
                        run.dropouts += 1;
                        prev_raw = 0.0;
                        silent_since = true;
                        continue;
                    }
                    // Inside a fade the index is scaled and names no frame; a
                    // sample counts only when it continues the one before it.
                    let continues = raw == prev_raw + 1.0;
                    prev_raw = raw;
                    if !continues {
                        continue;
                    }
                    let v = raw as usize;
                    if last_played > 0 && v > last_played + 1 {
                        let jump = v - last_played - 1;
                        run.skipped += jump;
                        run.worst_jump = run.worst_jump.max(jump);
                        if !silent_since {
                            run.worst_jump_heard = run.worst_jump_heard.max(jump);
                        }
                    }
                    last_played = v;
                    silent_since = false;
                    run.worst_delay = run.worst_delay.max(fed.saturating_sub(v));
                }
            } else {
                // Counting starts from the first frame played after settling.
                prev_raw = out[(block - 1) * 2];
                last_played = 0;
            }
            t_out += out_period;
        }
        run
    }

    #[test]
    fn dropouts_and_restarts_never_click() {
        // A 60 Hz sine through a locked live source whose capture pauses for
        // 300 ms and, separately, stalls for 20 ms and catches up. Wherever
        // audio stops or starts, it ramps: no sample-to-sample step beyond a
        // 32-frame fade of the full amplitude.
        let (valid, _) = passthrough_graph();
        let (mut built, mut producers) = build_with_block(Some("s"), SR, 64, &valid, SR, true);
        built.graph.lock_inputs(&HashSet::from(["m".to_string()]));
        let input = producers.get_mut("m").unwrap();
        let sine = |f: usize| (f as f32 * std::f32::consts::TAU * 60.0 / SR as f32).sin() * 0.9;
        let burst = 64usize;
        let (mut fed, mut t_out, mut due) = (0usize, 0.0f64, 0.0f64);
        let period_in = burst as f64 / SR as f64;
        let mut out = vec![0.0; 64 * 2];
        let mut played: Vec<f32> = Vec::new();
        let mut chunk = vec![0.0; burst * 2];
        while t_out < 4.0 {
            if due <= t_out {
                let paused = (1.5..1.8).contains(&due);
                if !paused {
                    for f in 0..burst {
                        let v = sine(fed + f);
                        chunk[f * 2] = v;
                        chunk[f * 2 + 1] = v;
                    }
                    push_all(input, &chunk);
                }
                fed += burst;
                let next = due + period_in;
                // A 20 ms stall at 3 s: deliveries wait, then arrive at once.
                due = if (3.0..3.02).contains(&next) {
                    3.02
                } else {
                    next
                };
                continue;
            }
            built.graph.process_block(&mut out);
            played.extend(out.iter().step_by(2));
            t_out += 64.0 / SR as f64;
        }
        let worst = played
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 0.9 / SPLICE_FADE_FRAMES as f32 + 0.01,
            "a {worst} step"
        );
    }

    #[test]
    fn a_jittery_tap_plays_every_frame() {
        for block in [32, 64, 256] {
            let r = tap_capture(block, 512, 60.0, 10.0, 3.0, &[]);
            assert_eq!(r.dropouts, 0, "{block}: dropouts");
            // Splices may trim drift, never whole stretches of audio.
            assert!(
                r.skipped < 50 * 64,
                "{block}: {} frames skipped in 50 s",
                r.skipped
            );
        }
    }

    #[test]
    fn a_stalled_tap_resumes_without_piling_up_latency() {
        // Every 5 s the tap holds its deliveries for 25 ms, then catches up.
        let stalls: Vec<(f64, f64)> = (0..12)
            .map(|k| (5.0 + k as f64 * 5.0, 5.025 + k as f64 * 5.0))
            .collect();
        let steady = tap_capture(32, 512, 65.0, 3.0, 1.0, &[]);
        let r = tap_capture(32, 512, 65.0, 3.0, 1.0, &stalls);
        // The stall is heard as a dropout. What arrived late for the time
        // already played as silence goes under that silence: never more than
        // the stall plus one delivery, and never while audio plays.
        let stall = SR as usize / 40 + 512;
        assert!(
            r.worst_jump <= stall,
            "{} frames vanished at once",
            r.worst_jump
        );
        assert_eq!(r.worst_jump_heard, 0, "audio skipped while playing");
        // A tap that stalls again within a minute gets a queue deep enough to
        // cover its stalls, once; nothing piles up beyond that.
        assert!(
            r.worst_delay <= steady.worst_delay + stall,
            "delay grew to {} frames from {}",
            r.worst_delay,
            steady.worst_delay
        );
    }

    #[test]
    fn reported_queue_tracks_the_real_delay() {
        let ticks = live_capture_with_gaps(64, 480, 55.0, &[(10.0, 15.0), (25.0, 30.0)], true);
        for k in ticks.iter().filter(|k| k.t > 2.0) {
            if let Some(real) = k.real_delay {
                assert!(
                    k.reported_queue <= real + 480,
                    "{:.2} s: reported {} frames queued, real delay {real}",
                    k.t,
                    k.reported_queue
                );
            }
        }
        let worst = ticks.iter().map(|k| k.reported_queue).max().unwrap();
        assert!(worst < 4 * 480, "queue readout reached {worst} frames");
    }

    #[test]
    fn live_source_runs_at_capture_latency_without_glitches() {
        for ppm in [-500.0, -200.0, 0.0, 200.0, 500.0] {
            let (out, stats, spliced) = live_capture(64, 512, ppm, 40.0, 5.0, |_| 0.5);
            let worst = out.iter().map(|s| (s - 0.5).abs()).fold(0.0_f32, f32::max);
            assert!(worst < 1e-3, "{ppm} ppm: constant signal off by {worst}");
            let queue = stats.queue_frames.load(Ordering::Relaxed) as usize;
            assert!(
                queue < 512 + 3 * 64 + 256,
                "{ppm} ppm: {queue} frames queued"
            );
            assert_eq!(stats.xrun.load(Ordering::Relaxed), 0, "{ppm} ppm: ran dry");
            assert_eq!(spliced, 0, "{ppm} ppm: spliced once settled");
        }
    }

    #[test]
    fn drift_is_absorbed_without_a_step() {
        // A 60 Hz sine's second difference never exceeds 0.9 * (2 pi 60 / SR)^2,
        // about 6e-5. The resampler moving its ratio by a few ppm keeps it
        // there; a splice, a repeated frame or a dropout is orders above.
        // 800 frames is one period: the phase stays exact however long it runs.
        let sine = |f: usize| ((f % 800) as f64 * std::f64::consts::TAU / 800.0).sin() as f32 * 0.9;
        let ideal = 0.9 * (std::f32::consts::TAU * 60.0 / SR as f32).powi(2);
        for block in [32, 256] {
            for ppm in [-500.0, -300.0, 300.0, 500.0] {
                let (out, stats, spliced) = live_capture(block, 480, ppm, 60.0, 5.0, sine);
                assert_eq!(spliced, 0, "{block}/{ppm} ppm: spliced");
                assert_eq!(
                    stats.xrun.load(Ordering::Relaxed),
                    0,
                    "{block}/{ppm} ppm: ran dry"
                );
                let worst = out
                    .windows(3)
                    .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
                    .fold(0.0_f32, f32::max);
                assert!(
                    worst < 4.0 * ideal,
                    "{block}/{ppm} ppm: second difference {worst}, a clean sine peaks at {ideal}"
                );
            }
        }
    }

    #[test]
    fn a_steered_source_never_touches_the_heap() {
        let (valid, _) = passthrough_graph();
        let (mut built, mut producers) = build_with_block(Some("s"), SR, 64, &valid, SR, true);
        let clock = Arc::new(WriteClock::default());
        built.graph.attach_write_clock("m", &clock);
        let input = producers.get_mut("m").unwrap();
        let chunk = vec![0.25_f32; 480 * 2];
        let mut out = vec![0.0; 64 * 2];
        let mut t = 0.0;
        // Warm up: prime, settle, run dry once and restart.
        for k in 0..2_000 {
            if k % 7 == 0 && !(900..960).contains(&k) {
                clock.record(chunk.len(), t);
                push_all(input, &chunk);
            }
            built.graph.process_block_at(&mut out, t);
            t += 64.0 / SR as f64;
        }
        crate::audio::rt_guard::assert_no_alloc("steered source", || {
            for k in 0..4_000 {
                if k % 7 == 0 && !(1_000..1_060).contains(&k) {
                    clock.record(chunk.len(), t);
                    push_all(input, &chunk);
                }
                built.graph.process_block_at(&mut out, t);
                t += 64.0 / SR as f64;
            }
        });
    }

    #[test]
    fn mono_mic_into_an_analyzer_renders() {
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                node("lm", NodeKind::LevelMeter, serde_json::json!({})),
            ],
            edges: vec![edge("e1", "m", None, "lm", None)],
        };
        let valid = g.validate().expect("valid");
        let native = HashMap::from([("m".to_string(), SR)]);
        let native_ch = HashMap::from([("m".to_string(), 1u32)]);
        let mut pairs = Vec::new();
        let mut reg = fresh_registry();
        let mut built = build_output_graph(
            None,
            SR,
            TIMER_BLOCK_FRAMES,
            true,
            &valid,
            &native,
            &native_ch,
            &mut pairs,
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            HashMap::new(),
            &HashMap::new(),
        )
        .expect("build");
        push_all(&mut pairs[0].1, &vec![0.5; 4 * TIMER_BLOCK_FRAMES]);
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        for _ in 0..4 {
            built.graph.process_block(&mut out);
        }
        let peaks = built.meters[0].snapshot_and_decay().peaks;
        assert_eq!(peaks.len(), 1, "one channel metered");
        assert!(peaks[0] > 0.4, "mono signal reached the meter: {peaks:?}");
    }

    #[test]
    fn missing_sample_rate_is_a_validation_error() {
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m"), speaker("s")],
            edges: vec![edge("e1", "m", None, "s", None)],
        };
        let valid = g.validate().expect("valid");
        let mut producer_pairs = Vec::new();
        let native = HashMap::new(); // no entry for "m"
        let native_ch = HashMap::new();
        let mut reg = fresh_registry();
        let err = build_output_graph(
            Some("s"),
            SR,
            TIMER_BLOCK_FRAMES,
            false,
            &valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            HashMap::new(),
            &HashMap::new(),
        );
        assert!(err.is_err(), "input without SR must fail");
    }

    #[test]
    fn monitor_graph_has_no_terminals_and_tracks_blocks() {
        let mut g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m"), speaker("s")],
            edges: vec![edge("e1", "m", None, "s", None)],
        };
        // An analyzer is a monitor-only node.
        g.nodes
            .push(node("lm", NodeKind::LevelMeter, serde_json::json!({})));
        g.edges.push(edge("e2", "m", None, "lm", None));
        let valid = g.validate().expect("valid");
        let (mut built, mut producers) = build(None, SR, &valid, SR, false);
        let prod = producers.get_mut("m").unwrap();
        push_all(prod, &stereo_ramp(4096, 0.0));
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out);
        assert_eq!(
            built
                .output
                .blocks
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        // Monitor has no terminals: the active width floors at 1.
        assert_eq!(built.graph.active_output_channels(), 1);
    }

    #[test]
    fn fan_out_effect_is_computed_once_and_shared_via_ring() {
        // mic → gain → spkrA, and gain → spkrB: the gain node is shared.
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m"), gain_node("g", 0.0), speaker("a"), speaker("b")],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "g", None, "a", None),
                edge("e3", "g", None, "b", None),
            ],
        };
        let valid = g.validate().expect("valid");

        let plan = plan_cuts(&valid, None);
        assert_eq!(
            plan.owner.get("g"),
            Some(&"a".to_string()),
            "first output owns"
        );
        let consumers = plan.consumers.get("g").expect("fan-out consumers");
        assert!(consumers.contains(&"b".to_string()));
        let participants = plan.participants();
        assert!(participants.contains("a") && participants.contains("b"));

        // Building B with the cut leaf: A's published block feeds B.
        let (mut built_a, mut producers_a) = build(Some("a"), SR, &valid, SR, false);
        let (node_idx, width) = built_a.node_meta["g"];
        assert_eq!(width, 2);
        let (prod, cons) = RingBuffer::<f32>::new(SR as usize * 2);
        let mut cut_leaves = HashMap::new();
        cut_leaves.insert("g".to_string(), (cons, SR, 2, Arc::default()));

        let mut producer_pairs = Vec::new();
        let native = valid.inputs.iter().map(|i| (i.id.clone(), SR)).collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let mut reg = fresh_registry();
        let mut built_b = build_output_graph(
            Some("b"),
            SR,
            TIMER_BLOCK_FRAMES,
            false,
            &valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            cut_leaves,
            &HashMap::new(),
        )
        .expect("build B");
        assert_eq!(built_b.sources.len(), 1, "the shared node is a ring source");
        assert_eq!(built_b.sources[0].channels, 2);

        // A publishes a block into the tap ring; B reads it back.
        built_a.graph.attach_tap(node_idx, prod, Arc::default());
        let published = stereo_ramp(1024, 0.0);
        push_all(producers_a.get_mut("m").unwrap(), &published);
        let mut out_a = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built_a.graph.process_block(&mut out_a);
        let mut out_b = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built_b.graph.process_block(&mut out_b);
        assert_eq!(out_b, out_a, "consumer reads the owner's published block");
    }

    #[test]
    fn net_sender_output_builds_a_consumer_node() {
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                node(
                    "net",
                    NodeKind::NetSender,
                    serde_json::json!({
                        "targetIp": "127.0.0.1",
                        "port": 9,
                        "codec": "pcm-f32",
                        "opusBitrate": 96_000,
                        "opusApplication": "audio",
                        "channels": 2
                    }),
                ),
            ],
            edges: vec![edge("e1", "m", None, "net", Some("ch1"))],
        };
        let valid = g.validate().expect("send graph valid");
        assert_eq!(valid.outputs.len(), 1);
        let (mut built, _) = build(Some("net"), SR, &valid, SR, false);
        assert_eq!(built.graph.out_channels(), 2);
        // Send-side graph is driven like any output.
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out);
    }

    #[test]
    fn file_recording_output_builds_terminals() {
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                node(
                    "rec",
                    NodeKind::FileRecording,
                    serde_json::json!({
                        "filePath": "/tmp/test.wav",
                        "format": { "kind": "wav", "bitDepth": "f32" },
                        "channels": 2,
                        "mode": "overwrite",
                        "sampleRate": 48000
                    }),
                ),
            ],
            edges: vec![edge("e1", "m", None, "rec", None)],
        };
        let valid = g.validate().expect("recording graph valid");
        let (mut built, _) = build(Some("rec"), SR, &valid, SR, false);
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out);
        assert_eq!(out, vec![0.0; TIMER_BLOCK_FRAMES * 2]);
    }

    #[test]
    fn multi_input_sums_into_the_mix() {
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m1"), mic("m2"), speaker("s")],
            edges: vec![
                edge("e1", "m1", None, "s", None),
                edge("e2", "m2", None, "s", None),
            ],
        };
        let valid = g.validate().expect("valid");
        let mut producer_pairs = Vec::new();
        let native = valid.inputs.iter().map(|i| (i.id.clone(), SR)).collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let mut reg = fresh_registry();
        let mut built = build_output_graph(
            Some("s"),
            SR,
            TIMER_BLOCK_FRAMES,
            false,
            &valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            HashMap::new(),
            &HashMap::new(),
        )
        .expect("build");
        let mut a = stereo_ramp(4096, 0.0);
        let mut b = stereo_ramp(4096, 0.5);
        push_all(&mut producer_pairs[0].1, &a);
        push_all(&mut producer_pairs[1].1, &b);
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out);
        built.graph.process_block(&mut out);
        built.graph.process_block(&mut out);
        for i in 0..out.len() {
            let want = a[2048 * 2 + i] + b[2048 * 2 + i];
            assert!(
                (out[i] - want).abs() < 1e-4,
                "sample {i}: {} vs {}",
                out[i],
                want
            );
        }
        let _ = &mut a;
        let _ = &mut b;
    }

    #[test]
    fn real_rate_change_resamples_and_reports_meta() {
        let (valid, _) = passthrough_graph();
        let (built, _) = build(Some("s"), SR, &valid, 44_100, true);
        let meta = &built.sources[0];
        assert_eq!(meta.native_sr, 44_100);
        assert_eq!(meta.channels, 2);
        // 1024 frames @ 48k consume ceil(1024 * 44100 / 48000) = 941 @ 44.1k.
        assert_eq!(meta.frames_per_block, 941);
    }

    #[test]
    fn set_out_channels_updates_width() {
        let (valid, _) = passthrough_graph();
        let (mut built, _) = build(Some("s"), SR, &valid, SR, false);
        assert_eq!(built.graph.out_channels(), 2);
        built.graph.set_out_channels(8);
        assert_eq!(built.graph.out_channels(), 8);
    }

    #[cfg(test)]
    mod helper_tests {
        use super::*;

        #[test]
        fn staging_ring_roundtrips_and_wraps() {
            let mut r = StagingRing::with_capacity(8);
            assert_eq!(r.len(), 0);
            r.extend_from_slice(&[1.0, 2.0, 3.0, 4.0]);
            let mut dst = vec![0.0; 2];
            assert_eq!(r.pop_into(&mut dst), 2);
            assert_eq!(dst, vec![1.0, 2.0]);
            // Wrap past the end.
            r.extend_from_slice(&[5.0, 6.0, 7.0]);
            let mut dst = vec![0.0; 3];
            assert_eq!(r.pop_into(&mut dst), 3);
            assert_eq!(dst, vec![3.0, 4.0, 5.0]);
            r.clear();
            assert_eq!(r.len(), 0);
        }

        #[test]
        fn staging_ring_capacity_is_respected() {
            // In debug builds an overrun is a debug_assert! panic: the clamp only
            // exists as a release-mode safety net. Tests must stay within cap.
            let mut r = StagingRing::with_capacity(4);
            r.extend_from_slice(&[1.0, 2.0]);
            r.extend_from_slice(&[3.0, 4.0]);
            assert_eq!(r.len(), 4);
            assert_eq!(r.dropped(), 0);
            let mut dst = vec![0.0; 4];
            assert_eq!(r.pop_into(&mut dst), 4);
            assert_eq!(dst, vec![1.0, 2.0, 3.0, 4.0]);
        }

        #[test]
        fn partial_pop_is_idempotent() {
            let mut r = StagingRing::with_capacity(8);
            r.extend_from_slice(&[1.0, 2.0, 3.0]);
            let mut dst = vec![0.0; 5];
            assert_eq!(r.pop_into(&mut dst), 3);
            assert_eq!(&dst[..3], &[1.0, 2.0, 3.0]);
            assert_eq!(r.len(), 0);
            assert_eq!(r.pop_into(&mut dst), 0);
        }

        #[test]
        fn handle_parsing() {
            assert_eq!(parse_ch("ch3"), Some(3));
            assert_eq!(parse_ch("ch"), None);
            assert_eq!(parse_ch("st2"), None);
            assert_eq!(parse_stereo("st2"), Some(2));
            assert_eq!(parse_stereo("ch3"), None);
            assert_eq!(tap_handle_width("ch3"), Some(1));
            assert_eq!(tap_handle_width("st2"), Some(2));
            assert_eq!(tap_handle_width("mix"), None);
            assert_eq!(target_route("ch4"), Some((3, 1)));
            assert_eq!(target_route("st2"), Some((1, 2)));
            assert_eq!(target_route("other"), None);
        }

        #[test]
        fn tap_keys_map_wire_names() {
            let TapKey::Channel(k) = tap_key("ch1").expect("ch1") else {
                panic!("variant")
            };
            assert_eq!(k, "0", "direct-IP wire channels are 0-based");
            let TapKey::Channel(k) = tap_key("ch12").expect("ch12") else {
                panic!("variant")
            };
            assert_eq!(k, "11");
            let TapKey::Channel(k) = tap_key("peer:p:0").expect("peer channel") else {
                panic!("variant")
            };
            assert_eq!(k, "p:0", "peer channel keys drop the peer: prefix");
            let TapKey::PrefixMix(p) = tap_key("peer:p").expect("peer mix") else {
                panic!("variant")
            };
            assert_eq!(p, "p:");
            assert!(tap_key("noise").is_none());
        }

        #[test]
        fn add_to_channel_downmixes_into_one_physical_channel() {
            let mut src = vec![0.0; TIMER_BLOCK_FRAMES * 2];
            for f in 0..TIMER_BLOCK_FRAMES {
                src[f * 2] = 1.0;
                src[f * 2 + 1] = 3.0;
            }
            let mut dst = vec![0.0; TIMER_BLOCK_FRAMES * 4];
            add_to_channel(&src, &mut dst, 2, TIMER_BLOCK_FRAMES);
            for f in 0..TIMER_BLOCK_FRAMES {
                assert_eq!(dst[f * 4 + 2], 2.0, "mono mean into channel 2");
                assert_eq!(dst[f * 4], 0.0, "other channels untouched");
            }
            // Out of range is a no-op.
            let mut small = vec![0.0; TIMER_BLOCK_FRAMES];
            add_to_channel(&src, &mut small, 5, TIMER_BLOCK_FRAMES);
            assert_eq!(small, vec![0.0; TIMER_BLOCK_FRAMES]);
        }

        #[test]
        fn add_block_at_places_a_stereo_pair() {
            let src = vec![0.5; TIMER_BLOCK_FRAMES * 2];
            let mut dst = vec![0.0; TIMER_BLOCK_FRAMES * 4];
            add_block_at(&src, &mut dst, 2, TIMER_BLOCK_FRAMES);
            for f in 0..TIMER_BLOCK_FRAMES {
                assert_eq!(dst[f * 4 + 2], 0.5);
                assert_eq!(dst[f * 4 + 3], 0.5);
                assert_eq!(dst[f * 4], 0.0);
            }
            // Offset past the end is a no-op.
            let mut small = vec![0.0; TIMER_BLOCK_FRAMES];
            add_block_at(&src, &mut small, 3, TIMER_BLOCK_FRAMES);
            assert_eq!(small, vec![0.0; TIMER_BLOCK_FRAMES]);
        }

        #[test]
        fn add_mapped_monotostereo_upmixes_every_channel() {
            let src = vec![0.5; TIMER_BLOCK_FRAMES];
            let mut dst = vec![0.0; TIMER_BLOCK_FRAMES * 4];
            add_mapped(&src, &mut dst, TIMER_BLOCK_FRAMES);
            for s in &dst {
                assert_eq!(*s, 0.5);
            }
            // Zero-width guards.
            let mut empty_dst: Vec<f32> = Vec::new();
            add_mapped(&[], &mut empty_dst, TIMER_BLOCK_FRAMES);
        }

        #[test]
        fn crossfade_into_blends_the_second_half() {
            let mut dst = vec![1.0_f32; SPLICE_FADE_FRAMES * 2];
            let incoming = vec![0.0_f32; SPLICE_FADE_FRAMES * 2];
            crossfade_into(&mut dst, &incoming, &[], 2);
            assert_eq!(dst[0], 1.0, "first frame stays pure outgoing");
            assert_eq!(dst[dst.len() - 1], 0.0, "last frame is pure incoming");
            // Incoming longer than dst is truncated safely.
            let mut dst = vec![1.0_f32; SPLICE_FADE_FRAMES * 2];
            let mut incoming = vec![0.0_f32; SPLICE_FADE_FRAMES * 2];
            incoming.extend(vec![0.5_f32; 1000]);
            crossfade_into(&mut dst, &incoming, &[], 2);
            assert!(dst.iter().all(|s| s.is_finite()));
            assert_eq!(
                *dst.last().unwrap(),
                0.0,
                "extra incoming beyond dst is ignored"
            );
        }

        #[test]
        fn delay_line_zero_capacity_passes_through() {
            let mut line = DelayLine::new(0, 2, TIMER_BLOCK_FRAMES);
            let input = vec![0.7f32; 16];
            assert_eq!(line.delayed(&input), input);
        }

        #[test]
        fn source_stats_start_at_zero() {
            let s = SourceStats::new();
            assert_eq!(s.xrun.load(Ordering::Relaxed), 0);
            assert_eq!(s.stalled.load(Ordering::Relaxed), 0);
            assert_eq!(s.trimmed.load(Ordering::Relaxed), 0);
            assert_eq!(s.consumed.load(Ordering::Relaxed), 0);
            assert_eq!(s.level.load(Ordering::Relaxed), 0);
        }

        #[test]
        fn ring_capacity_is_one_second() {
            assert_eq!(ring_capacity_frames(48_000), 48_000);
        }
    }

    #[test]
    fn empty_ring_zero_fills_and_counts_xruns() {
        let (valid, _) = passthrough_graph();
        let (mut built, _) = build(Some("s"), SR, &valid, SR, false);
        let meta = built.sources[0].stats.clone();
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        // Fresh source: not stalled yet → genuine xrun.
        built.graph.process_block(&mut out);
        assert_eq!(out, vec![0.0; TIMER_BLOCK_FRAMES * 2]);
        assert_eq!(
            meta.xrun.load(std::sync::atomic::Ordering::Relaxed),
            (TIMER_BLOCK_FRAMES * 2) as u64
        );
        // After the stall window the same silence counts as "stalled".
        std::thread::sleep(STALL_THRESHOLD + std::time::Duration::from_millis(20));
        built.graph.process_block(&mut out);
        assert_eq!(
            meta.stalled.load(std::sync::atomic::Ordering::Relaxed),
            (TIMER_BLOCK_FRAMES * 2) as u64
        );
        assert_eq!(
            meta.xrun.load(std::sync::atomic::Ordering::Relaxed),
            (TIMER_BLOCK_FRAMES * 2) as u64,
            "stall must not double-count as xrun"
        );
    }

    #[test]
    fn backlog_trims_with_a_splice_rather_than_a_step() {
        // Realtime source with more than HIGH (4 blocks) backlog: the trim
        // path kicks in, drops the excess over blocks and counts it.
        let (valid, _) = passthrough_graph();
        let (mut built, mut producers) = build(Some("s"), SR, &valid, SR, true);
        let meta = built.sources[0].stats.clone();
        // 12 blocks of backlog >> the 4-block high watermark.
        push_all(
            producers.get_mut("m").unwrap(),
            &stereo_ramp(12 * 1024, 0.0),
        );
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        for _ in 0..6 {
            built.graph.process_block(&mut out);
        }
        assert!(
            meta.trimmed.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "backlog must be trimmed"
        );
        assert!(
            meta.consumed.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "consumed counter tracks reads"
        );
        assert!(out.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn per_channel_taps_carry_single_channels() {
        // mic --ch1--> speaker: the terminal taps only the left channel.
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m"), speaker("s")],
            edges: vec![edge("e1", "m", Some("ch1"), "s", None)],
        };
        let valid = g.validate().expect("valid");
        let (mut built, mut producers) = build(Some("s"), SR, &valid, SR, false);
        // Asymmetric content: L=10, R=-10 so the tap is verifiable.
        let mut fed = vec![0.0; 2048 * 2];
        for f in 0..2048 {
            fed[f * 2] = 10.0;
            fed[f * 2 + 1] = -10.0;
        }
        push_all(producers.get_mut("m").unwrap(), &fed);
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out);
        // The stereo terminal gets a mono source: add_mapped upmixes the
        // tapped left channel to both physical channels.
        for f in 0..TIMER_BLOCK_FRAMES {
            assert_eq!(out[f * 2], 10.0, "left");
            assert_eq!(out[f * 2 + 1], 10.0, "mono upmix of the tapped channel");
        }
    }

    #[test]
    fn target_route_places_the_pair_at_its_offset() {
        // gain --st3--> speaker: the output lands on physical channels 2-3.
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m"), gain_node("g", 0.0), speaker("s")],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "g", Some("st3"), "s", None),
            ],
        };
        let valid = g.validate().expect("valid");
        let (mut built, mut producers) = build(Some("s"), SR, &valid, SR, false);
        push_all(producers.get_mut("m").unwrap(), &stereo_ramp(4096, 0.0));
        let mut out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built.graph.process_block(&mut out);
        built.graph.process_block(&mut out);
        // st3 asks for channels 2-3 of a 2-wide output → clamped to nothing.
        assert_eq!(out, vec![0.0; TIMER_BLOCK_FRAMES * 2]);
    }

    #[test]
    fn sidechain_edge_feeds_the_detector_not_the_mix() {
        let graph = |with_sidechain: bool| GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("main"),
                mic("key"),
                node(
                    "c",
                    NodeKind::Compressor,
                    serde_json::json!({
                        "thresholdDb": -30.0, "ratio": 8.0, "attackMs": 1.0,
                        "releaseMs": 50.0, "kneeDb": 0.0, "makeupDb": 0.0
                    }),
                ),
                speaker("s"),
            ],
            edges: if with_sidechain {
                vec![
                    edge("e1", "main", None, "c", None),
                    edge("e2", "key", None, "c", Some("sidechain")),
                    edge("e3", "c", None, "s", None),
                ]
            } else {
                vec![
                    edge("e1", "main", None, "c", None),
                    edge("e3", "c", None, "s", None),
                ]
            },
        };

        let valid = graph(true).validate().expect("sidechain graph");
        let (mut keyed, mut keyed_producers) = build(Some("s"), SR, &valid, SR, false);
        push_all(
            keyed_producers.get_mut("main").unwrap(),
            &vec![0.1; 8 * TIMER_BLOCK_FRAMES * 2],
        );
        push_all(
            keyed_producers.get_mut("key").unwrap(),
            &vec![1.0; 8 * TIMER_BLOCK_FRAMES * 2],
        );

        let valid = graph(false).validate().expect("main-only graph");
        let (mut unkeyed, mut unkeyed_producers) = build(Some("s"), SR, &valid, SR, false);
        push_all(
            unkeyed_producers.get_mut("main").unwrap(),
            &vec![0.1; 8 * TIMER_BLOCK_FRAMES * 2],
        );

        let mut keyed_out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        let mut unkeyed_out = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        for _ in 0..4 {
            keyed.graph.process_block(&mut keyed_out);
            unkeyed.graph.process_block(&mut unkeyed_out);
        }
        let keyed_peak = keyed_out.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        let unkeyed_peak = unkeyed_out.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!(unkeyed_peak > 0.02, "main signal vanished: {unkeyed_peak}");
        assert!(
            keyed_peak < unkeyed_peak * 0.3,
            "sidechain did not drive compression: keyed {keyed_peak}, main-only {unkeyed_peak}"
        );
        assert!(
            keyed_peak < 0.1,
            "sidechain audio leaked into the main mix: {keyed_peak}"
        );
    }

    #[test]
    fn plan_cuts_gives_shared_nodes_to_the_speaker() {
        // The recorder is listed first, yet the speaker computes the shared
        // gain at its small block and the recorder reads it back.
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                gain_node("g", 0.0),
                node(
                    "rec",
                    NodeKind::FileRecording,
                    serde_json::json!({
                        "filePath": "/tmp/test.wav",
                        "format": { "kind": "wav", "bitDepth": "f32" },
                        "channels": 2,
                        "mode": "overwrite",
                        "sampleRate": 48000
                    }),
                ),
                speaker("s"),
            ],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "g", None, "rec", None),
                edge("e3", "g", None, "s", None),
            ],
        };
        let valid = g.validate().expect("valid");
        assert_eq!(valid.outputs[0].id, "rec", "recorder listed first");
        let plan = plan_cuts(&valid, None);
        assert_eq!(plan.owner.get("g"), Some(&"s".to_string()));
        assert_eq!(plan.consumers.get("g"), Some(&vec!["rec".to_string()]));
    }

    #[test]
    fn plan_cuts_monitor_owns_analyzer_only_nodes() {
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![
                mic("m"),
                gain_node("g", 0.0),
                speaker("s"),
                node("lm", NodeKind::LevelMeter, serde_json::json!({})),
            ],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "g", None, "s", None),
                edge("e3", "g", None, "lm", None),
            ],
        };
        let valid = g.validate().expect("valid");
        let plan = plan_cuts(&valid, Some("monitor"));
        // The gain is owned by the speaker output; the monitor reads it back.
        assert_eq!(plan.owner.get("g"), Some(&"s".to_string()));
        assert!(plan
            .consumers
            .get("g")
            .map(|c| c.contains(&"monitor".to_string()))
            .unwrap_or(false));
        assert!(plan.participants().contains("monitor"));
    }

    #[test]
    fn plan_cuts_participants_exclude_ownerless_nodes() {
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m"), speaker("s")],
            edges: vec![edge("e1", "m", None, "s", None)],
        };
        let valid = g.validate().expect("valid");
        let plan = plan_cuts(&valid, None);
        // No effect nodes → no cuts, no participants.
        assert!(plan.owner.is_empty());
        assert!(plan.participants().is_empty());
    }

    #[test]
    fn reachable_backward_walks_the_whole_chain() {
        let (valid, _) = passthrough_graph();
        let r = reachable_backward("s", &valid);
        assert!(r.contains("m") && r.contains("g"));
        assert!(!r.contains("s"));
        let inputs = inputs_feeding_output("s", &valid);
        assert_eq!(inputs, vec!["m"]);
    }

    #[test]
    fn cut_leaf_across_rates_resamples() {
        // Owner runs at 44.1k, the consumer output at 48k → ring_source
        // gets a resampler and the published block arrives resampled.
        let g = GraphSpec {
            sample_rate: None,
            buffer_frames: None,
            nodes: vec![mic("m"), gain_node("g", 0.0), speaker("a"), speaker("b")],
            edges: vec![
                edge("e1", "m", None, "g", None),
                edge("e2", "g", None, "a", None),
                edge("e3", "g", None, "b", None),
            ],
        };
        let valid = g.validate().expect("valid");
        let (mut built_a, mut producers_a) = build(Some("a"), 44_100, &valid, 44_100, false);
        let (node_idx, _) = built_a.node_meta["g"];
        let (prod, cons) = RingBuffer::<f32>::new(SR as usize * 2);
        let mut cut_leaves = HashMap::new();
        cut_leaves.insert("g".to_string(), (cons, 44_100, 2, Arc::default()));

        let mut producer_pairs = Vec::new();
        let native = valid
            .inputs
            .iter()
            .map(|i| (i.id.clone(), 44_100))
            .collect();
        let native_ch = valid.inputs.iter().map(|i| (i.id.clone(), 2u32)).collect();
        let mut reg = fresh_registry();
        let mut built_b = build_output_graph(
            Some("b"),
            SR,
            TIMER_BLOCK_FRAMES,
            false,
            &valid,
            &native,
            &native_ch,
            &mut producer_pairs,
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            cut_leaves,
            &HashMap::new(),
        )
        .expect("build B");
        assert_eq!(built_b.sources[0].native_sr, 44_100);

        // Feed a constant-amplitude ramp at 44.1k; publish into the tap ring
        // exactly what A's effect produced.
        push_all(producers_a.get_mut("m").unwrap(), &stereo_ramp(4096, 0.0));
        let mut out_a = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built_a.graph.attach_tap(node_idx, prod, Arc::default());
        built_a.graph.process_block(&mut out_a);
        // A block published at 44.1k resampled to 48k: the value must land
        // in B's output, finite and near the source level.
        let mut out_b = vec![0.0; TIMER_BLOCK_FRAMES * 2];
        built_b.graph.process_block(&mut out_b);
        let peak = out_b.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let consumed = built_b.sources[0]
            .stats
            .consumed
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(consumed > 0, "B's cut source never read the ring");
        assert!(
            peak > 0.1,
            "B must receive the owner's audio resampled to 48k, peak {peak}"
        );
        assert!(out_b.iter().all(|s| s.is_finite()));
    }
}
