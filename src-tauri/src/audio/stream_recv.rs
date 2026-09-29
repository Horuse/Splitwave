//! Shared receive path for network audio (WebRTC and direct-IP).
//!
//! Pull-driven ASRC design: the decode task pushes decoded 48 kHz audio
//! *event-driven* (on packet arrival) straight into each consumer's per-channel
//! ring -- there is no fan-out timer and no second buffer at a different clock.
//! Each output consumer resamples on its own audio-clock thread with a
//! fixed-OUTPUT resampler (one block per callback), and a slow PI loop
//! (`drift_loop`) nudges the resample ratio to hold the ring near a target
//! fill, reading it against the packets' write timing so their 20 ms arrival
//! saw is not mistaken for drift. So clock drift is absorbed continuously by
//! the resampler, never by dropping/inserting samples -- which is what
//! produced the "needle" discontinuities before.
//!
//! Channels of one source stay in phase by position, not by arrival: each push
//! carries the `seq` it ends at, losses are pushed as concealment of the exact
//! size they replace, and a ring wired in mid-stream opens with the silence that
//! puts it where its siblings already are. Everything downstream then only has
//! to keep consumption equal across the source (`GroupPlan`).

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::audio::adaptive_depth::{DepthEstimator, OutageJudge};
use crate::audio::drift_loop::{ArrivalClock, DriftLoop};
use crate::audio::health;
use crate::audio::input_bridge::{now_secs, WriteClock};
use crate::audio::resample::MultiResamplerOut;
use crate::audio::streams::bulk_push_counted;

/// Decoded network audio is always carried at 48 kHz, one channel per stream.
pub const SR: u32 = 48_000;
/// Per-consumer, per-channel 48 kHz jitter ring (~2 s mono) -- headroom for a
/// deep adaptive target plus a burst after a latency spike.
pub const CONSUMER_RING: usize = 96_000;

/// Jitter-buffer depth (mono samples at 48 kHz) the drift loop steers the
/// fill toward and the buffer primes to. Sized by `DepthEstimator` from the
/// arrival jitter actually measured; these bound it.
const TARGET_INIT: usize = 2_880; // ~60 ms, until the first second is measured
const TARGET_MIN: usize = 960; // ~20 ms: one packet of the default codec
const TARGET_MAX: usize = 19_200; // ~400 ms
/// Kept above the measured jitter.
const TARGET_SAFETY: usize = 240; // ~5 ms
/// One jitter measurement spans this long: many packets, so one late packet
/// shows as a dip rather than as the whole window.
const JITTER_WINDOW_MS: f64 = 500.0;
/// A backlog jump beyond this is a re-prime refill, not drift.
const TARGET_EVENT_MAX: usize = 4_800; // ~100 ms
/// The depth primed from `TARGET_INIT` is corrected once, when measured, by
/// splices of at most this many samples under a crossfade of `SPLICE_FADE`,
/// one per `SPLICE_EVERY_MS` at most.
const SPLICE_MAX: usize = 64;
const SPLICE_FADE: usize = 32;
const SPLICE_EVERY_MS: usize = 21;

/// Time without a single new sample before a channel counts as gone (the same
/// window the DSP sources call a stall). A channel the sender never transmits
/// still has a tap here -- the UI can wire a handle the peer doesn't fill, and
/// a channel that stops is never reaped -- and a group decision taken over it
/// would stall every sibling forever.
const IDLE_DEAD_MS: usize = 170;

/// One received channel's fan-out: the 48 kHz ring producer for each live
/// consumer. The decode task pushes decoded audio into all of them.
pub type ChannelBroadcast = Arc<ChannelFeed>;

pub struct ChannelFeed {
    /// Shared by every channel of the same source (see `group_id`), and held
    /// for the whole of a push or a ring creation, so a ring being wired in
    /// never lands inside a push.
    sync: Arc<Mutex<()>>,
    prods: Mutex<Vec<Producer<f32>>>,
    state: Mutex<FeedState>,
    pub sample_rate: Arc<AtomicU32>,
    /// When the decode task pushed, for the consumers' drift loops.
    clock: Arc<WriteClock>,
}

#[derive(Default)]
struct FeedState {
    /// Packet index just past the last one pushed, on the source's shared
    /// timeline (all its channels are encoded from one tick, so `seq` counts
    /// the same instants for each). Extended past the 16-bit wire counter.
    pos_end: u64,
    /// Samples one packet carries, from the last push.
    chunk: usize,
}

/// Widen a 16-bit wire counter to the epoch of `near`.
fn extend_seq(seq: u16, near: u64) -> u64 {
    let delta = seq.wrapping_sub(near as u16) as i16;
    near.wrapping_add(delta as i64 as u64)
}

/// A consumer's per-channel playback taps, keyed by channel id.
pub type TapMap = Arc<Mutex<HashMap<String, PlaybackTap>>>;

/// Handle returned when registering an output consumer.
pub struct ConsumerHandle {
    pub taps: TapMap,
    pub drift: Arc<AtomicU32>,
    pub target: Arc<AtomicU32>,
    pub realtime: bool,
    /// Output frames the consumer's graph renders per block.
    pub block_frames: usize,
    pub output_rate: u32,
}

/// Push one channel's audio for the `packets` packets ending at `seq` (a lost
/// packet is carried as its concealment, so a push always covers whole packets).
pub fn broadcast_push(broadcast: &ChannelBroadcast, seq: u16, packets: u16, samples: &[f32]) {
    broadcast_push_sr(broadcast, seq, packets, samples, SR);
}

/// Push one channel's audio with the sample rate the packet carried.
pub fn broadcast_push_sr(
    broadcast: &ChannelBroadcast,
    seq: u16,
    packets: u16,
    samples: &[f32],
    sample_rate: u32,
) {
    broadcast_push_at(broadcast, seq, packets, samples, sample_rate, now_secs());
}

/// `broadcast_push_sr` at `at` seconds on `input_bridge::now_secs`'s clock.
fn broadcast_push_at(
    broadcast: &ChannelBroadcast,
    seq: u16,
    packets: u16,
    samples: &[f32],
    sample_rate: u32,
    at: f64,
) {
    broadcast.clock.record(samples.len(), at);
    if sample_rate > 0 {
        broadcast.sample_rate.store(sample_rate, Ordering::Relaxed);
    }
    let _sync = broadcast.sync.lock().unwrap();
    let mut prods = broadcast.prods.lock().unwrap();
    prods.retain(|p| !p.is_abandoned());
    for p in prods.iter_mut() {
        bulk_push_counted(p, samples, &health::NET_RING_OVERRUN_SAMPLES);
    }
    let mut state = broadcast.state.lock().unwrap();
    state.pos_end = extend_seq(seq, state.pos_end) + 1;
    if packets > 0 {
        state.chunk = samples.len() / packets as usize;
    }
}

/// Channels of one source (a WebRTC peer, or the whole direct-IP receiver) key
/// as `group:channel` / `channel`. They carry one recording's worth of
/// simultaneous audio, so every buffer decision has to be taken across the
/// group -- priming, discarding or concealing one channel alone shifts it in
/// time against its siblings, and that phase error combs the summed signal.
fn group_id(key: &str) -> u64 {
    let group = key.rsplit_once(':').map(|(g, _)| g).unwrap_or("");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in group.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// One channel's playback state for one consumer: a jitter ring plus a
/// fixed-output resampler (source rate -> consumer rate) whose ratio tracks drift.
pub struct PlaybackTap {
    group: u64,
    consumer: Consumer<f32>,
    resampler: MultiResamplerOut,
    rate: u32,
    current_source_sr: u32,
    source_sr: Arc<AtomicU32>,
    base_ratio: f64,
    last_ratio: f64,
    realtime: bool,
    drift: Arc<AtomicU32>,
    clock: Arc<WriteClock>,
    in_buf: Vec<f32>,
    pub scratch: Vec<f32>,
    pub valid: usize,
    primed: bool,
    // Fill at the top of the current block. Levelling reads it rather than the
    // live count, which the decode task keeps growing while we walk the map.
    snap_backlog: usize,
    // Samples popped so far plus the current fill: a total that only moves when
    // the decode task delivered something, so idle blocks are countable.
    popped: u64,
    last_total: u64,
    idle_blocks: u32,
    idle_dead_blocks: u32,
    /// Samples delivered since the previous block.
    arrived: usize,
    // PLC: the last real output block, and whether we're currently in a gap.
    // On a network underrun we fade this out (instead of a hard silence step),
    // and fade the real audio back in on recovery.
    last_block: Vec<f32>,
    gap: bool,
}

impl PlaybackTap {
    #[allow(clippy::too_many_arguments)]
    fn new(
        group: u64,
        consumer: Consumer<f32>,
        rate: u32,
        block_frames: usize,
        realtime: bool,
        primed: bool,
        drift: Arc<AtomicU32>,
        source_sr: Arc<AtomicU32>,
        clock: Arc<WriteClock>,
    ) -> Self {
        let in_sr = source_sr.load(Ordering::Relaxed).max(1);
        let base_ratio = rate as f64 / in_sr as f64;
        let resampler =
            MultiResamplerOut::new(in_sr, rate, block_frames, 1).expect("mono resampler init");
        Self {
            group,
            consumer,
            resampler,
            rate,
            current_source_sr: in_sr,
            source_sr,
            base_ratio,
            last_ratio: base_ratio,
            realtime,
            drift,
            clock,
            in_buf: Vec::with_capacity(4096 + SPLICE_MAX),
            scratch: Vec::with_capacity(block_frames),
            valid: 0,
            primed,
            snap_backlog: 0,
            popped: 0,
            last_total: 0,
            idle_blocks: 0,
            arrived: 0,
            idle_dead_blocks: (IDLE_DEAD_MS * rate as usize / 1000 / block_frames.max(1)).max(2)
                as u32,
            last_block: vec![0.0; block_frames],
            gap: false,
        }
    }

    fn backlog(&self) -> usize {
        self.consumer.slots()
    }

    /// Input samples the next resample will consume (reflects the current ratio).
    fn need_in(&self) -> usize {
        self.resampler.input_frames_next()
    }

    /// Whether this channel has gone quiet long enough to be left out of its
    /// group's decisions.
    fn dead(&self) -> bool {
        self.idle_blocks >= self.idle_dead_blocks
    }

    /// Fold this block's arrivals into the idle count. Call once per block,
    /// after `snap_backlog` is taken.
    fn track_liveness(&mut self) {
        let total = self.popped + self.snap_backlog as u64;
        self.arrived = total.saturating_sub(self.last_total) as usize;
        if total != self.last_total {
            self.last_total = total;
            self.idle_blocks = 0;
            return;
        }
        self.idle_blocks = self.idle_blocks.saturating_add(1);
        // Drop out of the primed set on the way out, so coming back means
        // re-priming with the group and picking its alignment up again.
        if self.idle_blocks == self.idle_dead_blocks {
            self.primed = false;
        }
    }

    /// Discard `n` buffered samples. The group applies the same count to every
    /// channel, so a splice never costs them their alignment.
    fn trim(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let n = n.min(self.consumer.slots());
        if let Ok(chunk) = self.consumer.read_chunk(n) {
            chunk.commit_all();
            self.popped += n as u64;
            self.gap = true;
        }
    }

    /// Emit silence for this block without touching the ring (the group is
    /// priming).
    fn hold(&mut self) -> usize {
        self.valid = 0;
        0
    }

    /// Produce one output block into `scratch` (valid = its length), resampling
    /// 48 kHz -> consumer rate at the drift-adjusted ratio, `splice` samples
    /// cut out of it under a crossfade into the block's tail. Returns the
    /// sample count (0 = emit silence after a sustained network underrun).
    fn fill_block(&mut self, splice: usize) -> usize {
        let in_sr = self.source_sr.load(Ordering::Relaxed);
        if in_sr != 0 && in_sr != self.current_source_sr {
            self.current_source_sr = in_sr;
            self.base_ratio = self.rate as f64 / in_sr as f64;
            self.last_ratio = self.base_ratio;
            if let Ok(r) = MultiResamplerOut::new(in_sr, self.rate, self.last_block.len(), 1) {
                self.resampler = r;
            }
        }
        // Track drift ratio for this block.
        if self.realtime {
            let d = f32::from_bits(self.drift.load(Ordering::Relaxed)) as f64;
            let ratio = self.base_ratio * d;
            if (ratio - self.last_ratio).abs() > 1e-9 {
                self.resampler.set_ratio(ratio);
                self.last_ratio = ratio;
            }
        }
        let need = self.need_in();
        if self.consumer.slots() < need {
            // Real network underrun: conceal (fade), don't step to silence.
            return self.conceal();
        }
        let splice = if self.consumer.slots() >= need + splice {
            splice
        } else {
            0
        };

        self.in_buf.clear();
        if let Ok(chunk) = self.consumer.read_chunk(need + splice) {
            let (a, b) = chunk.as_slices();
            self.in_buf.extend_from_slice(a);
            self.in_buf.extend_from_slice(b);
            chunk.commit_all();
            self.popped += (need + splice) as u64;
        }
        if splice > 0 {
            // The block's tail blends into the stream `splice` further on, so
            // it ends exactly where the next block picks up.
            let fade = SPLICE_FADE.min(need);
            for i in 0..fade {
                let j = need - fade + i;
                let w = (i + 1) as f32 / fade as f32;
                self.in_buf[j] = self.in_buf[j] * (1.0 - w) + self.in_buf[j + splice] * w;
            }
            self.in_buf.truncate(need);
        }
        self.scratch.clear();
        if self
            .resampler
            .process(&self.in_buf, &mut self.scratch)
            .is_err()
        {
            return self.conceal();
        }
        // Keep the real (full-amplitude) block for future concealment.
        if self.last_block.len() == self.scratch.len() {
            self.last_block.copy_from_slice(&self.scratch);
        }
        if self.gap {
            // Recovery: fade the real audio in over this block so the join off
            // the concealed tail has no step.
            let frames = self.scratch.len();
            for (i, s) in self.scratch.iter_mut().enumerate() {
                *s *= (i as f32 + 1.0) / frames as f32;
            }
            self.gap = false;
        }
        self.valid = self.scratch.len();
        self.valid
    }

    /// Concealment for a missing block: on entering a gap, one fade-out of the
    /// last real block; a sustained gap drops back to priming so the ring
    /// refills to target in silence -- resuming shallow would leave every
    /// jitter ripple causing another underrun (audible fade cycling), and the
    /// narrow drift clamp can't rebuild depth.
    fn conceal(&mut self) -> usize {
        if self.gap {
            self.primed = false;
            self.valid = 0;
            return 0;
        }
        self.gap = true;
        self.scratch.clear();
        self.scratch.extend_from_slice(&self.last_block);
        let frames = self.scratch.len();
        for (i, s) in self.scratch.iter_mut().enumerate() {
            *s *= 1.0 - (i as f32 + 1.0) / frames as f32;
        }
        self.valid = self.scratch.len();
        self.valid
    }
}

/// Registry mapping each received channel to its per-consumer rings. The decode
/// task pushes into the broadcasts; each consumer owns a `TapMap`.
pub struct FanoutRegistry {
    broadcasts: Mutex<HashMap<String, ChannelBroadcast>>,
    consumers: Mutex<Vec<ConsumerRef>>,
    /// One push lock per source, handed to every channel of it.
    groups: Mutex<HashMap<u64, Arc<Mutex<()>>>>,
}

struct ConsumerRef {
    rate: u32,
    block_frames: usize,
    realtime: bool,
    drift: Arc<AtomicU32>,
    target: Arc<AtomicU32>,
    taps: Weak<Mutex<HashMap<String, PlaybackTap>>>,
}

impl Default for FanoutRegistry {
    fn default() -> Self {
        Self {
            broadcasts: Mutex::new(HashMap::new()),
            consumers: Mutex::new(Vec::new()),
            groups: Mutex::new(HashMap::new()),
        }
    }
}

impl FanoutRegistry {
    fn group_sync(&self, gid: u64) -> Arc<Mutex<()>> {
        self.groups
            .lock()
            .unwrap()
            .entry(gid)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// New output consumer: an empty tap map wired a fresh ring into every known
    /// channel's broadcast. Locks `consumers` before `broadcasts`.
    pub fn register_consumer(
        &self,
        output_sr: u32,
        block_frames: usize,
        realtime: bool,
    ) -> ConsumerHandle {
        let map: TapMap = Arc::new(Mutex::new(HashMap::new()));
        let drift = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let target = Arc::new(AtomicU32::new(TARGET_INIT as u32));
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|c| c.taps.strong_count() > 0);
        // A source's rings are created together under its push lock, so they all
        // open empty at the same instant. Wiring them one packet apart would
        // offset the channels against each other for the consumer's lifetime.
        let broadcasts = self.broadcasts.lock().unwrap();
        let mut by_group: HashMap<u64, Vec<(&String, &ChannelBroadcast)>> = HashMap::new();
        for (key, bc) in broadcasts.iter() {
            by_group.entry(group_id(key)).or_default().push((key, bc));
        }
        let mut pad: Vec<f32> = Vec::new();
        for (gid, channels) in by_group {
            let sync = channels[0].1.sync.clone();
            let _sync = sync.lock().unwrap();
            // Within one tick some channels have already been pushed and some
            // have not. Opening every ring at the least advanced position and
            // padding the rest by how far they lead puts them all on the same
            // instant, whatever order the packets landed in.
            let states: Vec<(u64, usize)> = channels
                .iter()
                .map(|(_, bc)| {
                    let st = bc.state.lock().unwrap();
                    (st.pos_end, st.chunk)
                })
                .collect();
            // One packet behind the furthest: within a tick the channels differ
            // by at most that, and opening there costs at most one packet of
            // depth while keeping them in phase whatever the arrival order. A
            // channel further behind has stopped delivering, and re-enters in
            // phase through `drop_channel` when it comes back.
            let head = states.iter().map(|(pos, _)| *pos).max().unwrap_or(0);
            let base = head.saturating_sub(1);
            for ((key, bc), (pos_end, chunk)) in channels.into_iter().zip(states) {
                let (mut prod, cons) = RingBuffer::<f32>::new(CONSUMER_RING);
                let lead = pos_end.saturating_sub(base).min(1) as usize * chunk;
                if lead > 0 {
                    pad.clear();
                    pad.resize(lead.min(CONSUMER_RING / 2), 0.0);
                    bulk_push_counted(&mut prod, &pad, &health::NET_RING_OVERRUN_SAMPLES);
                }
                bc.prods.lock().unwrap().push(prod);
                map.lock().unwrap().insert(
                    key.clone(),
                    PlaybackTap::new(
                        gid,
                        cons,
                        output_sr,
                        block_frames,
                        realtime,
                        false,
                        drift.clone(),
                        bc.sample_rate.clone(),
                        bc.clock.clone(),
                    ),
                );
            }
        }
        drop(broadcasts);
        consumers.push(ConsumerRef {
            rate: output_sr,
            block_frames,
            realtime,
            drift: drift.clone(),
            target: target.clone(),
            taps: Arc::downgrade(&map),
        });
        ConsumerHandle {
            taps: map,
            drift,
            target,
            realtime,
            block_frames,
            output_rate: output_sr,
        }
    }

    /// New received channel, wired into every live consumer. `first_seq` is the
    /// packet about to be pushed: its distance from the siblings' position is
    /// what the fresh rings open with, so the channel joins in phase.
    pub fn attach_channel(&self, key: String, first_seq: u16) -> ChannelBroadcast {
        let gid = group_id(&key);
        let sync = self.group_sync(gid);
        let bc: ChannelBroadcast = Arc::new(ChannelFeed {
            sync: sync.clone(),
            prods: Mutex::new(Vec::new()),
            state: Mutex::new(FeedState::default()),
            sample_rate: Arc::new(AtomicU32::new(SR)),
            clock: Arc::new(WriteClock::default()),
        });
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|c| c.taps.strong_count() > 0);
        // No sibling can push while the rings are sized and wired, so the
        // positions and fills read here are the ones the new rings must match.
        let _sync = sync.lock().unwrap();
        let sibling = {
            let broadcasts = self.broadcasts.lock().unwrap();
            broadcasts
                .iter()
                .filter(|(k, _)| group_id(k) == gid)
                .map(|(k, b)| {
                    let st = b.state.lock().unwrap();
                    (k.clone(), st.pos_end, st.chunk)
                })
                .max_by_key(|(_, pos_end, _)| *pos_end)
        };
        let mut pad: Vec<f32> = Vec::new();
        for c in consumers.iter() {
            if let Some(map) = c.taps.upgrade() {
                let (mut prod, cons) = RingBuffer::<f32>::new(CONSUMER_RING);
                let mut taps = map.lock().unwrap();
                // A sibling's ring spans [read position, its last packet]; the
                // new channel starts at `first_seq`, so it opens with whatever
                // separates the two.
                let (fill, primed) = sibling
                    .as_ref()
                    .and_then(|(k, _, _)| taps.get(k))
                    .map(|t| (t.backlog() as i64, t.primed))
                    .unwrap_or((0, false));
                let lead = sibling
                    .as_ref()
                    .map(|(_, pos_end, chunk)| {
                        let start = extend_seq(first_seq, *pos_end);
                        (start as i64 - *pos_end as i64) * *chunk as i64
                    })
                    .unwrap_or(0);
                let open = (fill + lead).clamp(0, (CONSUMER_RING / 2) as i64) as usize;
                if open > 0 {
                    pad.clear();
                    pad.resize(open, 0.0);
                    bulk_push_counted(&mut prod, &pad, &health::NET_RING_OVERRUN_SAMPLES);
                }
                bc.prods.lock().unwrap().push(prod);
                taps.insert(
                    key.clone(),
                    PlaybackTap::new(
                        gid,
                        cons,
                        c.rate,
                        c.block_frames,
                        c.realtime,
                        primed,
                        c.drift.clone(),
                        bc.sample_rate.clone(),
                        bc.clock.clone(),
                    ),
                );
            }
        }
        self.broadcasts.lock().unwrap().insert(key, bc.clone());
        bc
    }

    /// Deepest jitter buffer target any live consumer of this source is steering
    /// to, in 48 kHz samples. The drift loop holds the actual fill there, so it
    /// is the latency the receive path adds.
    pub fn buffer_depth(&self) -> Option<u32> {
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|c| c.taps.strong_count() > 0);
        consumers
            .iter()
            .map(|c| c.target.load(Ordering::Relaxed))
            .max()
    }

    /// Forgets one channel. Its next packet re-attaches it, which is how a
    /// stream that broke for longer than concealment covers gets back in phase.
    pub fn drop_channel(&self, key: &str) {
        self.broadcasts.lock().unwrap().remove(key);
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|c| c.taps.strong_count() > 0);
        for c in consumers.iter() {
            if let Some(map) = c.taps.upgrade() {
                map.lock().unwrap().remove(key);
            }
        }
    }

    /// Drops channels whose key starts with `prefix`.
    pub fn drop_prefix(&self, prefix: &str) {
        self.broadcasts
            .lock()
            .unwrap()
            .retain(|k, _| !k.starts_with(prefix));
    }

    pub fn clear(&self) {
        self.broadcasts.lock().unwrap().clear();
        self.consumers.lock().unwrap().clear();
        self.groups.lock().unwrap().clear();
    }
}

/// RT-side reader over a channel tap map, shared by every node that emits
/// received audio (WebRTC bridge, direct-IP receiver). Owns the per-block
/// resample + summing and, for a real-time consumer, the drift controller.
pub struct ChannelReceiver {
    taps: TapMap,
    drift: Arc<AtomicU32>,
    target: Arc<AtomicU32>,
    realtime: bool,
    block_frames: usize,
    // Adaptive-jitter state; only the single audio thread touches these.
    depth: std::cell::RefCell<DepthEstimator>,
    judge: std::cell::RefCell<OutageJudge>,
    /// Source samples one block consumes.
    need: usize,
    starving: Cell<bool>,
    /// Fill a refilling source must reach before it plays: the target plus
    /// the arrival saw's dip, since priming ends right after a packet.
    start: Cell<usize>,
    ever_primed: Cell<bool>,
    /// The startup depth correction has been decided; what is left of it.
    settled: Cell<bool>,
    owed: Cell<usize>,
    cooldown: Cell<usize>,
    cooldown_blocks: usize,
    drift_loop: std::cell::RefCell<DriftLoop>,
    /// Delivery timing of the source the loop follows.
    arrival: std::cell::RefCell<ArrivalClock>,
    /// That source and its rate. Following another, or the same one after it
    /// refilled, starts the loop settling afresh.
    steered: Cell<Option<(u64, u32)>>,
    /// Smoothed fill of the emptiest playing source, in 48 kHz samples: the
    /// latency the receive buffer adds. A gauge for the latency report.
    fill: Arc<AtomicU32>,
    fill_avg: Cell<f64>,
    // Last emitted mix, held when the tap map is briefly locked for registration
    // so a lock miss is an inaudible repeat rather than a silent click.
    last_mix: std::cell::RefCell<Vec<f32>>,
    // Per-block scratch for the group decisions; reused so `mix_block` never
    // allocates on the DSP thread.
    plans: std::cell::RefCell<Vec<GroupPlan>>,
}

/// One source's buffer decision for this block, taken over all its channels.
struct GroupPlan {
    id: u64,
    min_backlog: usize,
    /// The emptiest channel's write count and time, and its rate.
    written: (u64, f64),
    rate: u32,
    /// Primed this block, after filling from empty.
    refilled: bool,
    /// Least any channel of the source received since the last block.
    arrived: usize,
    need: usize,
    primed: bool,
    trim: usize,
    /// Samples to splice out of this block under a crossfade.
    splice: usize,
    hold: bool,
    conceal: bool,
}

impl ChannelReceiver {
    pub fn new(handle: ConsumerHandle) -> Self {
        // The fill is counted in 48 kHz source samples; one block takes the
        // output block's worth of them.
        let need = handle.block_frames * SR as usize / handle.output_rate.max(1) as usize;
        Self {
            taps: handle.taps,
            drift: handle.drift,
            target: handle.target,
            realtime: handle.realtime,
            block_frames: handle.block_frames,
            depth: std::cell::RefCell::new(DepthEstimator::new(
                SR,
                need.max(1),
                JITTER_WINDOW_MS,
                TARGET_INIT,
            )),
            judge: std::cell::RefCell::new(OutageJudge::new(SR)),
            need: need.max(1),
            starving: Cell::new(false),
            start: Cell::new(TARGET_INIT),
            ever_primed: Cell::new(false),
            settled: Cell::new(false),
            owed: Cell::new(0),
            cooldown: Cell::new(0),
            cooldown_blocks: (SPLICE_EVERY_MS * SR as usize / 1000 / need.max(1)).max(1),
            drift_loop: std::cell::RefCell::new(DriftLoop::new(SR, need.max(1))),
            arrival: std::cell::RefCell::new(ArrivalClock::new(SR)),
            steered: Cell::new(None),
            fill: Arc::new(AtomicU32::new(0)),
            fill_avg: Cell::new(0.0),
            last_mix: std::cell::RefCell::new(vec![0.0; handle.block_frames * 2]),
            plans: std::cell::RefCell::new(Vec::with_capacity(8)),
        }
    }

    /// Resample one block from every tap into its `scratch` and sum into `mix`.
    /// Real-time consumers also adapt the buffer depth and drift ratio here.
    #[cfg(test)]
    pub fn mix_block(&self, mix: &mut [f32]) {
        self.mix_block_at(mix, now_secs());
    }

    /// `mix_block` at `now` seconds on `input_bridge::now_secs`'s clock.
    pub fn mix_block_at(&self, mix: &mut [f32], now: f64) {
        let frames = self.block_frames;
        // The node's width is whatever its graph resolved to, not always stereo.
        let width = (mix.len() / frames).max(1);
        // The playback resamplers are built for exactly one block of output.
        debug_assert_eq!(mix.len() / width, frames);
        let Ok(mut taps) = self.taps.try_lock() else {
            // Map briefly locked for registration: hold the last mix rather than
            // emit a silent click.
            let held = self.last_mix.borrow();
            if held.len() == mix.len() {
                mix.copy_from_slice(&held);
            } else {
                mix.fill(0.0);
            }
            return;
        };
        mix.fill(0.0);

        // Collapse the taps into one plan per source, then act on every channel
        // of a source identically -- see `group_id`.
        let mut plans = self.plans.borrow_mut();
        plans.clear();
        for tap in taps.values_mut() {
            let backlog = tap.backlog();
            let need = tap.need_in();
            tap.snap_backlog = backlog;
            tap.track_liveness();
            if tap.dead() {
                continue;
            }
            match plans.iter_mut().find(|p| p.id == tap.group) {
                Some(p) => {
                    if backlog < p.min_backlog {
                        p.written = tap.clock.read();
                        p.rate = tap.current_source_sr;
                    }
                    p.min_backlog = p.min_backlog.min(backlog);
                    p.arrived = p.arrived.min(tap.arrived);
                    p.need = p.need.max(need);
                    p.primed &= tap.primed;
                }
                None => plans.push(GroupPlan {
                    id: tap.group,
                    min_backlog: backlog,
                    written: tap.clock.read(),
                    rate: tap.current_source_sr,
                    refilled: false,
                    arrived: tap.arrived,
                    need,
                    primed: tap.primed,
                    trim: 0,
                    splice: 0,
                    hold: false,
                    conceal: false,
                }),
            }
        }

        let target = self.target.load(Ordering::Relaxed) as usize;
        for p in plans.iter_mut() {
            if !p.primed {
                // Prime the whole source at once. Fills are not levelled: a
                // channel whose packet for this tick has already landed leads
                // its siblings by exactly that packet, and that lead *is* the
                // alignment.
                if p.min_backlog < self.start.get().max(p.need) {
                    p.hold = true;
                    continue;
                }
                p.primed = true;
                p.refilled = true;
            }
            // One channel short of a block stalls the whole source: a channel
            // that popped while a sibling concealed would sit a block ahead of
            // it for good.
            if p.min_backlog < p.need {
                p.conceal = true;
                continue;
            }
            // Safety net only (abnormal burst): the drift loop normally keeps
            // the ring near target. Generous headroom -- sender catch-up bursts
            // after a scheduler stall are legitimate and must not get spliced.
            let hard_cap = (target * 2).max(target + TARGET_EVENT_MAX * 4);
            if p.min_backlog > hard_cap {
                p.trim = p.min_backlog - target;
            }
        }

        if self.realtime {
            // Drive from the emptiest channel so no channel is left to underrun;
            // one shared ratio keeps channels phase-coherent. A priming source
            // never steers: its ring is filling by design, and reading that as
            // drift would chase the refill.
            let live = plans
                .iter()
                .filter(|p| !p.hold)
                .min_by_key(|p| p.min_backlog);
            // A source refilling after a break emits nothing, exactly like one
            // that concealed, and the target has to answer for both. Only the
            // first prime of all is exempt: filling from empty is how playback
            // starts, not something the buffer failed at.
            // A source whose every channel went quiet has no plan at all; that
            // silence is part of the outage too.
            let starving = live.is_some_and(|p| p.min_backlog < p.need)
                || (self.ever_primed.get() && (plans.is_empty() || plans.iter().any(|p| p.hold)));
            // What the emptiest source delivered, holding or not: the judge
            // needs the flow while a gap refills, too.
            let arrived = plans
                .iter()
                .min_by_key(|p| p.min_backlog)
                .map_or(0, |p| p.arrived);
            self.account(starving, live.map(|p| (p.min_backlog, p.need)), arrived);
            if let Some(p) = live {
                let avg = self.fill_avg.get();
                let avg = avg + (p.min_backlog as f64 - avg) * 0.02;
                self.fill_avg.set(avg);
                self.fill.store(avg.round() as u32, Ordering::Relaxed);
            }
            let live = live.map(|p| p.id);
            match plans.iter_mut().find(|p| Some(p.id) == live) {
                Some(p) if !p.conceal => p.splice = self.steer(p, now),
                Some(_) => {}
                None => self.steered.set(None),
            }
        }
        if plans.iter().any(|p| !p.hold) {
            self.ever_primed.set(true);
        }

        // Channels are mono; a wider mix gets each one centred across it.
        for tap in taps.values_mut() {
            if tap.dead() {
                tap.hold();
                continue;
            }
            let Some(plan) = plans.iter().find(|p| p.id == tap.group) else {
                tap.hold();
                continue;
            };
            if plan.hold {
                tap.hold();
                continue;
            }
            if plan.conceal {
                let n = tap.conceal().min(frames);
                for (frame, &v) in mix.chunks_mut(width).zip(tap.scratch[..n].iter()) {
                    for s in frame.iter_mut() {
                        *s += v;
                    }
                }
                continue;
            }
            tap.primed = true;
            tap.trim(plan.trim);
            let n = tap.fill_block(plan.splice).min(frames);
            for (frame, &v) in mix.chunks_mut(width).zip(tap.scratch[..n].iter()) {
                for s in frame.iter_mut() {
                    *s += v;
                }
            }
        }
        let mut held = self.last_mix.borrow_mut();
        if held.len() != mix.len() {
            held.resize(mix.len(), 0.0);
        }
        held.copy_from_slice(mix);
    }

    /// Adaptive buffer depth. `live` is the emptiest source that is actually
    /// playing, absent while every source is refilling; `arrived` is what the
    /// emptiest source delivered this block.
    ///
    /// Running dry deepens the buffer only by audio that turns out to have
    /// been late: a sender that goes quiet (a paused peer, Opus DTX) sends
    /// nothing because there is nothing, and waiting longer would not help.
    fn account(&self, starving: bool, live: Option<(usize, usize)>, arrived: usize) {
        let mut depth = self.depth.borrow_mut();
        let mut judge = self.judge.borrow_mut();
        if starving && !self.starving.get() {
            judge.missing(self.need);
            // The window holds the drain, which says nothing about jitter.
            depth.discard_window();
        }
        self.starving.set(starving);
        if let Some(late) = judge.delivered(arrived, self.need) {
            if late > 0 {
                depth.underrun(late);
            }
        }
        if starving {
            return;
        }
        let Some((backlog, need)) = live else { return };
        depth.observe(backlog);
        let dip = depth.depth();
        let target = (need + dip + TARGET_SAFETY).clamp(TARGET_MIN, TARGET_MAX);
        self.target.store(target as u32, Ordering::Relaxed);
        self.start.set((target + dip).min(TARGET_MAX));
    }

    /// Drift ratio for the emptiest playing source. Its fill plus what its
    /// sender has made but not yet pushed is the fill right after a packet,
    /// the top of the arrival saw, which is where priming leaves it: the loop
    /// holds it at the start level. Returns the samples to splice out of this
    /// block, which only the one startup correction ever asks for; the loop
    /// holds still while it runs.
    fn steer(&self, p: &GroupPlan, now: f64) -> usize {
        let mut drift = self.drift_loop.borrow_mut();
        let mut arrival = self.arrival.borrow_mut();
        let following = self.steered.get();
        if p.refilled || following != Some((p.id, p.rate)) {
            if following.map(|(_, rate)| rate) != Some(p.rate) {
                *arrival = ArrivalClock::new(p.rate);
            }
            arrival.reset();
            drift.restart();
            self.steered.set(Some((p.id, p.rate)));
        }
        arrival.observe(p.written.0, p.written.1);
        let start = self.start.get() as f64;
        let error = p.min_backlog as f64 + arrival.pending(now, start) - start;
        if !self.settled.get() {
            if !self.depth.borrow().is_measured() {
                drift.restart();
                return 0;
            }
            self.settled.set(true);
            self.owed.set(error.max(0.0).round() as usize);
        }
        if self.owed.get() > 0 {
            // The queue moves by design meanwhile; a window holding that
            // would read it as jitter.
            self.depth.borrow_mut().discard_window();
            drift.restart();
            let wait = self.cooldown.get();
            if wait > 0 {
                self.cooldown.set(wait - 1);
                return 0;
            }
            let n = self.owed.get().min(SPLICE_MAX);
            if p.min_backlog < p.need + n {
                return 0;
            }
            self.owed.set(self.owed.get() - n);
            self.cooldown.set(self.cooldown_blocks);
            return n;
        }
        let u = drift.update(error);
        let d = 1.0 / (1.0 + u);
        self.drift.store((d as f32).to_bits(), Ordering::Relaxed);
        0
    }

    /// Smoothed receive-buffer fill in 48 kHz samples, for the latency report.
    pub fn fill_gauge(&self) -> Arc<AtomicU32> {
        self.fill.clone()
    }

    /// Copy one channel's already-resampled scratch into `out`.
    pub fn channel(&self, key: &str, out: &mut [f32]) {
        out.fill(0.0);
        let Ok(taps) = self.taps.try_lock() else {
            return;
        };
        let Some(tap) = taps.get(key) else { return };
        let src = &tap.scratch[..tap.valid];
        // Taps are mono; a wider destination gets the channel centred across it.
        let width = out.len() / self.block_frames;
        if width <= 1 {
            let n = src.len().min(out.len());
            out[..n].copy_from_slice(&src[..n]);
        } else {
            for (frame, &v) in out.chunks_mut(width).zip(src.iter()) {
                frame.fill(v);
            }
        }
    }

    /// Sum every channel whose key starts with `prefix` into `out`.
    pub fn prefix_mix(&self, prefix: &str, out: &mut [f32]) {
        out.fill(0.0);
        if let Ok(taps) = self.taps.try_lock() {
            let width = (out.len() / self.block_frames).max(1);
            for (key, tap) in taps.iter() {
                if key.starts_with(prefix) {
                    for (frame, &v) in out.chunks_mut(width).zip(tap.scratch[..tap.valid].iter()) {
                        for s in frame.iter_mut() {
                            *s += v;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: usize = 1024;

    /// Registry + stereo consumer primed with `frames` of constant audio on
    /// both channels. Returns the receiver to mix and the broadcast handles.
    fn primed_registry(frames: usize, realtime: bool) -> (FanoutRegistry, ChannelReceiver) {
        let reg = FanoutRegistry::default();
        let seq: u16 = 100;
        // Consumers register FIRST: a push with no live consumer goes nowhere.
        let handle = reg.register_consumer(48_000, BLOCK, realtime);
        for key in ["0", "1"] {
            let bc = reg.attach_channel(key.to_string(), seq);
            broadcast_push(&bc, seq + 7, 8, &vec![0.5f32; frames]);
        }
        let recv = ChannelReceiver::new(handle);
        (reg, recv)
    }

    struct NetRun {
        /// Output samples after settling that were not the pushed constant.
        gaps: usize,
        target: usize,
        target_after_outage: usize,
        /// Largest ring fill seen after settling.
        max_fill: usize,
    }

    /// One 48 kHz channel of 20 ms packets of 0.5 into a consumer rendering
    /// `block` frames per callback. Packet `k` lands `jitter(k)` seconds late
    /// (in order); `outage` silences the sender for a span of time.
    fn net_sim(
        block: usize,
        seconds: f64,
        settle: f64,
        jitter: impl Fn(u32) -> f64,
        outage: Option<(f64, f64)>,
    ) -> NetRun {
        net_sim_with(block, seconds, settle, jitter, outage, &[])
    }

    /// Steady packets, except nothing at all is sent inside `quiet`.
    fn net_sim_quiet(block: usize, seconds: f64, settle: f64, quiet: &[(f64, f64)]) -> NetRun {
        net_sim_with(block, seconds, settle, |_| 0.0, None, quiet)
    }

    fn net_sim_with(
        block: usize,
        seconds: f64,
        settle: f64,
        jitter: impl Fn(u32) -> f64,
        outage: Option<(f64, f64)>,
        quiet: &[(f64, f64)],
    ) -> NetRun {
        const PACKET: usize = 960;
        let reg = FanoutRegistry::default();
        let handle = reg.register_consumer(48_000, block, true);
        let target = handle.target.clone();
        let bc = reg.attach_channel("0".into(), 0);
        let recv = ChannelReceiver::new(handle);
        let period = PACKET as f64 / SR as f64;
        let out_period = block as f64 / SR as f64;
        let mut run = NetRun {
            gaps: 0,
            target: 0,
            target_after_outage: 0,
            max_fill: 0,
        };
        let (mut k, mut t) = (0u32, 0.0);
        let mut mix = vec![0.0f32; block];
        let packet = vec![0.5f32; PACKET];
        while t < seconds {
            let due = k as f64 * period + jitter(k);
            if due <= t {
                let sent = k as f64 * period;
                let dropped = outage.is_some_and(|(a, b)| sent >= a && sent < b)
                    || quiet.iter().any(|&(a, b)| sent >= a && sent < b);
                if !dropped {
                    broadcast_push_at(&bc, k as u16, 1, &packet, SR, due);
                }
                k += 1;
                continue;
            }
            recv.mix_block_at(&mut mix, t);
            let in_outage = outage.is_some_and(|(a, b)| t >= a && t < b + 1.0);
            if t > settle && !in_outage {
                run.gaps += mix.iter().filter(|s| (*s - 0.5).abs() > 1e-3).count();
                let fill = recv
                    .taps
                    .lock()
                    .unwrap()
                    .get("0")
                    .map_or(0, |t| t.backlog());
                run.max_fill = run.max_fill.max(fill);
            }
            if let Some((_, b)) = outage {
                if t >= b + 0.3 && run.target_after_outage == 0 {
                    run.target_after_outage = target.load(Ordering::Relaxed) as usize;
                }
            }
            t += out_period;
        }
        run.target = target.load(Ordering::Relaxed) as usize;
        run
    }

    /// Deterministic pseudo-random share in [0, 1) per packet.
    fn noise(k: u32) -> f64 {
        let mut x = k.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
        x ^= x >> 15;
        x = x.wrapping_mul(0x2C1B_3C6D);
        x ^= x >> 12;
        (x % 10_000) as f64 / 10_000.0
    }

    #[test]
    fn steady_link_settles_below_the_initial_depth() {
        for block in [64, 256, 1024] {
            let r = net_sim(block, 30.0, 10.0, |_| 0.0, None);
            assert_eq!(r.gaps, 0, "{block}: gaps on a clean link");
            assert!(
                r.target < TARGET_INIT,
                "{block}: clean link still buffers {} samples",
                r.target
            );
        }
    }

    #[test]
    fn a_drifting_sender_is_absorbed_without_gaps_or_buildup() {
        // 300 ppm is far beyond real crystal error: two minutes drift ~1700
        // samples, all of which the ratio has to take up.
        for ppm in [-300.0, 300.0] {
            let drift = move |k: u32| -(k as f64 * 0.02) * ppm * 1e-6;
            let steady = net_sim(64, 120.0, 30.0, |_| 0.0, None);
            let r = net_sim(64, 120.0, 30.0, drift, None);
            assert_eq!(r.gaps, 0, "{ppm} ppm: gaps");
            assert!(
                r.max_fill <= steady.max_fill + 240,
                "{ppm} ppm: fill reached {} against {} steady",
                r.max_fill,
                steady.max_fill
            );
        }
    }

    #[test]
    fn the_startup_depth_is_corrected_quickly_and_seamlessly() {
        // A 60 Hz sine: one period is 800 samples, so its phase is exact.
        // Clean, it moves < 0.0071 per sample; a hard 64-sample cut would
        // jump it by up to 0.45.
        const PACKET: usize = 960;
        let sine = |n: usize| ((n % 800) as f64 * std::f64::consts::TAU / 800.0).sin() as f32 * 0.9;
        let block = 64;
        let reg = FanoutRegistry::default();
        let handle = reg.register_consumer(48_000, block, true);
        let bc = reg.attach_channel("0".into(), 0);
        let recv = ChannelReceiver::new(handle);
        let (mut k, mut t) = (0u32, 0.0);
        let mut mix = vec![0.0f32; block];
        let mut packet = vec![0.0f32; PACKET];
        let mut played = Vec::new();
        let mut late_fill = 0;
        while t < 12.0 {
            let due = k as f64 * PACKET as f64 / SR as f64;
            if due <= t {
                for (i, s) in packet.iter_mut().enumerate() {
                    *s = sine(k as usize * PACKET + i);
                }
                broadcast_push_at(&bc, k as u16, 1, &packet, SR, due);
                k += 1;
                continue;
            }
            recv.mix_block_at(&mut mix, t);
            if t > 0.5 {
                played.extend_from_slice(&mix);
            }
            if t > 6.0 {
                let fill = recv
                    .taps
                    .lock()
                    .unwrap()
                    .get("0")
                    .map_or(0, |t| t.backlog());
                late_fill = late_fill.max(fill);
            }
            t += block as f64 / SR as f64;
        }
        let worst = played
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < 0.03, "a step of {worst}");
        // Primed from the 60 ms prior; within seconds of measuring, the
        // buffer holds about one packet plus the safety margin.
        assert!(
            late_fill <= PACKET + TARGET_MIN + TARGET_SAFETY,
            "still {late_fill} samples buffered"
        );
    }

    #[test]
    fn jittery_link_buys_depth_instead_of_gaps() {
        // Wi-Fi-like: every packet up to 40 ms late.
        let r = net_sim(64, 60.0, 20.0, |k| noise(k) * 0.040, None);
        assert_eq!(r.gaps, 0, "gaps once the depth has adapted");
        assert!(
            r.target >= 1_920,
            "40 ms of jitter needs >= 40 ms: {}",
            r.target
        );
        assert!(r.target <= 4 * 1_920, "over-buffered: {}", r.target);
    }

    #[test]
    fn a_network_stall_deepens_the_buffer_at_once() {
        // Packets sent during a 150 ms stall reach us together when it clears.
        let stall = |k: u32| {
            let sent = k as f64 * 0.02;
            if (8.0..8.15).contains(&sent) {
                8.15 - sent
            } else {
                0.0
            }
        };
        let before = net_sim(64, 7.9, 5.0, stall, None).target;
        let r = net_sim(64, 20.0, 5.0, stall, Some((8.15, 8.15)));
        assert!(
            r.target_after_outage >= before + 5_000,
            "150 ms of late audio must buy most of 150 ms: {before} -> {}",
            r.target_after_outage
        );
    }

    #[test]
    fn a_quiet_sender_between_phrases_keeps_the_buffer_shallow() {
        // DTX-like: 400 ms of speech, 250 ms of nothing sent, for a minute.
        let quiet: Vec<(f64, f64)> = (0..90)
            .map(|k| (5.0 + k as f64 * 0.65 + 0.4, 5.0 + k as f64 * 0.65 + 0.65))
            .collect();
        let steady = net_sim(64, 65.0, 10.0, |_| 0.0, None).target;
        let r = net_sim_quiet(64, 65.0, 10.0, &quiet);
        assert!(
            r.target <= steady + 480,
            "silence between phrases deepened the buffer: {steady} -> {}",
            r.target
        );
    }

    #[test]
    fn a_sender_pause_is_not_charged_as_jitter() {
        let r = net_sim(64, 30.0, 5.0, |_| 0.0, Some((8.0, 13.0)));
        assert!(
            r.target_after_outage < TARGET_INIT,
            "a 5 s pause left {} samples of depth",
            r.target_after_outage
        );
    }

    #[test]
    fn mixing_never_touches_the_heap() {
        for block in [32, 64, 512] {
            let reg = FanoutRegistry::default();
            let handle = reg.register_consumer(44_100, block, true);
            let bc = reg.attach_channel("0".into(), 0);
            let recv = ChannelReceiver::new(handle);
            broadcast_push(&bc, 30, 30, &vec![0.5f32; 960 * 30]);
            let mut mix = vec![0.0f32; block * 2];
            for _ in 0..8 {
                recv.mix_block(&mut mix);
            }
            crate::audio::rt_guard::assert_no_alloc(&format!("mix_block @ {block}"), || {
                for _ in 0..200 {
                    recv.mix_block(&mut mix);
                }
            });
        }
    }

    #[test]
    fn extend_seq_wraps_16bit_counters() {
        assert_eq!(extend_seq(0, 0), 0);
        assert_eq!(extend_seq(5, 10), 5, "goes backwards within half a epoch");
        assert_eq!(extend_seq(10, 5), 10);
        // 16-bit wrap: 0 is one past 65535.
        assert_eq!(extend_seq(0, 65_535), 65_536);
        assert_eq!(extend_seq(65_535, 65_536), 65_535);
    }

    #[test]
    fn group_id_differs_for_peers_same_for_channels() {
        assert_eq!(group_id("peer:abc:0"), group_id("peer:abc:1"));
        assert_ne!(group_id("peer:abc:0"), group_id("peer:xyz:0"));
        assert_eq!(
            group_id("0"),
            group_id("1"),
            "direct-IP channels share a group"
        );
    }

    #[test]
    fn register_consumer_opens_in_phase_taps() {
        let (reg, recv) = primed_registry(960 * 10, true);
        // Both channels registered and share the group's target depth.
        assert_eq!(
            recv.taps.lock().unwrap().len(),
            2,
            "both channels registered"
        );
        // The broadcast is still tracked by the registry.
        drop(reg);
    }

    #[test]
    fn realtime_consumer_mixes_after_prime() {
        let (_, recv) = primed_registry(960 * 40, true);
        let mut mix = vec![0.0f32; BLOCK * 2];
        recv.mix_block(&mut mix);
        let max = mix.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(max > 0.4, "primed tap must reach the mix: {max}");
        assert!(mix.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn unprimed_consumer_streams_silence() {
        let reg = FanoutRegistry::default();
        let bc = reg.attach_channel("ch".into(), 0);
        // Nothing pushed: a realtime consumer must stream silence, not panic.
        let handle = reg.register_consumer(48_000, BLOCK, true);
        let recv = ChannelReceiver::new(handle);
        let mut mix = vec![0.0f32; BLOCK * 2];
        recv.mix_block(&mut mix);
        assert!(mix.iter().all(|s| *s == 0.0));
        let _ = bc;
    }

    #[test]
    fn channel_taps_draw_per_channel_audio() {
        let reg = FanoutRegistry::default();
        let bc = reg.attach_channel("0".into(), 10);
        let handle = reg.register_consumer(48_000, BLOCK, true);
        // Prime well past the 60 ms target so the tap actually plays out.
        broadcast_push(&bc, 11, 1, &vec![0.7f32; 960 * 40]);
        let recv = ChannelReceiver::new(handle);
        let mut mix = vec![0.0f32; BLOCK * 2];
        recv.mix_block(&mut mix);
        let mut tap = vec![0.0f32; BLOCK];
        recv.channel("0", &mut tap);
        let peak = tap.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.6, "channel tap carries the pushed audio: {peak}");
    }

    #[test]
    fn unknown_channel_tap_is_silence_not_panic() {
        let reg = FanoutRegistry::default();
        let handle = reg.register_consumer(48_000, BLOCK, true);
        let recv = ChannelReceiver::new(handle);
        let mut tap = vec![0.0f32; BLOCK];
        recv.channel("ghost", &mut tap);
        assert!(tap.iter().all(|s| *s == 0.0));
        recv.prefix_mix("ghost:", &mut tap);
        assert!(tap.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn drop_channel_and_clear_reset_state() {
        let reg = FanoutRegistry::default();
        let bc = reg.attach_channel("0".into(), 1);
        let handle = reg.register_consumer(48_000, BLOCK, true);
        let taps = handle.taps.clone();
        broadcast_push(&bc, 2, 1, &vec![0.5f32; 960]);
        assert_eq!(taps.lock().unwrap().len(), 1);
        reg.drop_channel("0");
        assert_eq!(
            taps.lock().unwrap().len(),
            0,
            "dropped channel leaves the tap map"
        );
        // Re-attach works after a drop.
        let bc2 = reg.attach_channel("0".into(), 9);
        broadcast_push(&bc2, 10, 1, &vec![0.5f32; 960]);
        assert_eq!(taps.lock().unwrap().len(), 1);
        // clear() forgets consumers and broadcasts; the tap map itself is
        // consumer-owned and just goes quiet (no further pushes).
        reg.clear();
        assert_eq!(reg.buffer_depth(), None, "no consumers left after clear");
    }

    #[test]
    fn buffer_depth_reports_the_consumer_target() {
        let reg = FanoutRegistry::default();
        assert_eq!(reg.buffer_depth(), None, "no consumers yet");
        let bc = reg.attach_channel("c".into(), 1);
        broadcast_push_sr(&bc, 2, 1, &vec![0.0f32; 1920], 44_100);
        let _handle = reg.register_consumer(48_000, BLOCK, true);
        let depth = reg.buffer_depth().expect("consumer registered");
        assert_eq!(
            depth, TARGET_INIT as u32,
            "fresh consumer steers to the init depth"
        );
        assert_eq!(
            bc.sample_rate.load(Ordering::Relaxed),
            44_100,
            "push carries its rate"
        );
    }
}
