//! Shared receive path for network audio (WebRTC and direct-IP).
//!
//! Pull-driven ASRC design: the decode task pushes decoded 48 kHz audio
//! *event-driven* (on packet arrival) straight into each consumer's per-channel
//! ring -- there is no fan-out timer and no second buffer at a different clock.
//! Each output consumer resamples on its own audio-clock thread with a
//! fixed-OUTPUT resampler (one block per callback), and a slow proportional
//! loop nudges the resample ratio to hold the ring near a target fill. So clock
//! drift is absorbed continuously by the resampler, never by dropping/inserting
//! samples -- which is what produced the "needle" discontinuities before.
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

use crate::audio::resample::MultiResamplerOut;
use crate::audio::streams::bulk_push;

/// Decoded network audio is always carried at 48 kHz, one channel per stream.
pub const SR: u32 = 48_000;
/// Output frames produced per block; must equal the DSP worker's block size.
pub const OUT_BLOCK_FRAMES: usize = 1024;
/// Per-consumer, per-channel 48 kHz jitter ring (~2 s mono) -- headroom for a
/// deep adaptive target plus a burst after a latency spike.
pub const CONSUMER_RING: usize = 96_000;

/// Adaptive jitter-buffer target (mono samples at 48 kHz): the fill the drift
/// loop steers toward and the buffer primes to. Starts moderate, grows when the
/// network under-delivers (a latency spike drains it), shrinks slowly during
/// sustained calm -- so a periodic spike is absorbed after the first occurrence.
const TARGET_INIT: usize = 2_880; // ~60 ms
const TARGET_MIN: usize = 1_920; // ~40 ms
const TARGET_MAX: usize = 19_200; // ~400 ms
const TARGET_GROW: usize = 2_400; // +50 ms on underrun
const TARGET_SHRINK: usize = 480; // -10 ms per calm window
const CALM_WINDOW: u32 = 480; // ~10 s at ~21 ms/block

/// Proportional gain: backlog error (samples) -> ratio correction. A varying
/// resample ratio IS pitch modulation, so the loop must be far slower and far
/// narrower than the jitter it rides on: corrections stay within +-2000 ppm
/// (real oscillator pairs differ by <200 ppm) and jitter is absorbed by the
/// buffer, never by the ratio -- otherwise bursty links produce audible wow.
const DRIFT_KP: f64 = 1.0e-7;
const DRIFT_MIN: f64 = 0.998;
const DRIFT_MAX: f64 = 1.002;
/// EMA smoothing for the backlog the drift controller sees (~3 s time constant)
/// so it tracks real clock drift (slow) and ignores per-block fill ripple.
const DRIFT_BACKLOG_ALPHA: f64 = 0.007;
/// No correction while the smoothed backlog is this close to target.
const DRIFT_DEADBAND: f64 = 240.0;

/// Blocks without a single new sample before a channel counts as gone (~170 ms
/// at 1024 frames / 48 kHz, the same window the DSP sources call a stall). A
/// channel the sender never transmits still has a tap here -- the UI can wire a
/// handle the peer doesn't fill, and a channel that stops is never reaped -- and
/// a group decision taken over it would stall every sibling forever.
const IDLE_BLOCKS_DEAD: u32 = 8;

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
}

/// Push one channel's audio for the `packets` packets ending at `seq` (a lost
/// packet is carried as its concealment, so a push always covers whole packets).
/// Tracking where the samples sit on the timeline -- rather than when they
/// arrived -- is what keeps a channel wired in mid-stream in phase with its
/// siblings.
pub fn broadcast_push(broadcast: &ChannelBroadcast, seq: u16, packets: u16, samples: &[f32]) {
    let _sync = broadcast.sync.lock().unwrap();
    let mut prods = broadcast.prods.lock().unwrap();
    prods.retain(|p| !p.is_abandoned());
    for p in prods.iter_mut() {
        bulk_push(p, samples);
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

/// One channel's playback state for one consumer: a 48 kHz jitter ring plus a
/// fixed-output resampler (48 kHz -> consumer rate) whose ratio tracks drift.
pub struct PlaybackTap {
    group: u64,
    consumer: Consumer<f32>,
    resampler: MultiResamplerOut,
    base_ratio: f64,
    last_ratio: f64,
    realtime: bool,
    drift: Arc<AtomicU32>,
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
    // PLC: the last real output block, and whether we're currently in a gap.
    // On a network underrun we fade this out (instead of a hard silence step),
    // and fade the real audio back in on recovery.
    last_block: Vec<f32>,
    gap: bool,
}

impl PlaybackTap {
    fn new(
        group: u64,
        consumer: Consumer<f32>,
        rate: u32,
        realtime: bool,
        primed: bool,
        drift: Arc<AtomicU32>,
    ) -> Self {
        let base_ratio = rate as f64 / SR as f64;
        let resampler = MultiResamplerOut::new(SR, rate, OUT_BLOCK_FRAMES, 1)
            .expect("mono resampler init");
        Self {
            group,
            consumer,
            resampler,
            base_ratio,
            last_ratio: base_ratio,
            realtime,
            drift,
            in_buf: Vec::with_capacity(4096),
            scratch: Vec::with_capacity(OUT_BLOCK_FRAMES),
            valid: 0,
            primed,
            snap_backlog: 0,
            popped: 0,
            last_total: 0,
            idle_blocks: 0,
            last_block: vec![0.0; OUT_BLOCK_FRAMES],
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
        self.idle_blocks >= IDLE_BLOCKS_DEAD
    }

    /// Fold this block's arrivals into the idle count. Call once per block,
    /// after `snap_backlog` is taken.
    fn track_liveness(&mut self) {
        let total = self.popped + self.snap_backlog as u64;
        if total != self.last_total {
            self.last_total = total;
            self.idle_blocks = 0;
            return;
        }
        self.idle_blocks = self.idle_blocks.saturating_add(1);
        // Drop out of the primed set on the way out, so coming back means
        // re-priming with the group and picking its alignment up again.
        if self.idle_blocks == IDLE_BLOCKS_DEAD {
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
    /// 48 kHz -> consumer rate at the drift-adjusted ratio. Returns the sample
    /// count (0 = emit silence after a sustained network underrun).
    fn fill_block(&mut self) -> usize {
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

        self.in_buf.clear();
        if let Ok(chunk) = self.consumer.read_chunk(need) {
            let (a, b) = chunk.as_slices();
            self.in_buf.extend_from_slice(a);
            self.in_buf.extend_from_slice(b);
            chunk.commit_all();
            self.popped += need as u64;
        }
        self.scratch.clear();
        if self.resampler.process(&self.in_buf, &mut self.scratch).is_err() {
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
    pub fn register_consumer(&self, output_sr: u32, realtime: bool) -> ConsumerHandle {
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
                    bulk_push(&mut prod, &pad);
                }
                bc.prods.lock().unwrap().push(prod);
                map.lock().unwrap().insert(
                    key.clone(),
                    PlaybackTap::new(gid, cons, output_sr, realtime, false, drift.clone()),
                );
            }
        }
        drop(broadcasts);
        consumers.push(ConsumerRef {
            rate: output_sr,
            realtime,
            drift: drift.clone(),
            target: target.clone(),
            taps: Arc::downgrade(&map),
        });
        ConsumerHandle { taps: map, drift, target, realtime }
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
                    bulk_push(&mut prod, &pad);
                }
                bc.prods.lock().unwrap().push(prod);
                taps.insert(
                    key.clone(),
                    PlaybackTap::new(gid, cons, c.rate, c.realtime, primed, c.drift.clone()),
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
        self.broadcasts.lock().unwrap().retain(|k, _| !k.starts_with(prefix));
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
    // Adaptive-jitter state; only the single worker thread touches these.
    min_backlog: Cell<usize>,
    window_blocks: Cell<u32>,
    avg_backlog: Cell<f64>,
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
    need: usize,
    primed: bool,
    trim: usize,
    hold: bool,
    conceal: bool,
}

impl ChannelReceiver {
    pub fn new(handle: ConsumerHandle) -> Self {
        Self {
            taps: handle.taps,
            drift: handle.drift,
            target: handle.target,
            realtime: handle.realtime,
            min_backlog: Cell::new(usize::MAX),
            window_blocks: Cell::new(0),
            avg_backlog: Cell::new(-1.0),
            last_mix: std::cell::RefCell::new(Vec::new()),
            plans: std::cell::RefCell::new(Vec::with_capacity(8)),
        }
    }

    /// Resample one block from every tap into its `scratch` and sum into `mix`.
    /// Real-time consumers also adapt the buffer depth and drift ratio here.
    pub fn mix_block(&self, mix: &mut [f32]) {
        // The node's width is whatever its graph resolved to, not always stereo.
        let width = (mix.len() / OUT_BLOCK_FRAMES).max(1);
        // The playback resamplers are built for exactly OUT_BLOCK_FRAMES output.
        debug_assert_eq!(mix.len() / width, OUT_BLOCK_FRAMES);
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
                    p.min_backlog = p.min_backlog.min(backlog);
                    p.need = p.need.max(need);
                    p.primed &= tap.primed;
                }
                None => plans.push(GroupPlan {
                    id: tap.group,
                    min_backlog: backlog,
                    need,
                    primed: tap.primed,
                    trim: 0,
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
                if p.min_backlog < target.max(p.need) {
                    p.hold = true;
                    continue;
                }
                p.primed = true;
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
            let hard_cap = (target * 2).max(target + OUT_BLOCK_FRAMES * 20);
            if p.min_backlog > hard_cap {
                p.trim = p.min_backlog - target;
            }
        }

        if self.realtime {
            // Drive from the emptiest channel so no channel is left to underrun;
            // one shared ratio keeps channels phase-coherent.
            if let Some(p) = plans.iter().min_by_key(|p| p.min_backlog) {
                self.control(p.min_backlog, p.need);
            }
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
                let n = tap.conceal().min(OUT_BLOCK_FRAMES);
                for (frame, &v) in mix.chunks_mut(width).zip(tap.scratch[..n].iter()) {
                    for s in frame.iter_mut() {
                        *s += v;
                    }
                }
                continue;
            }
            tap.primed = true;
            tap.trim(plan.trim);
            let n = tap.fill_block().min(OUT_BLOCK_FRAMES);
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

    /// Adaptive buffer depth + drift ratio from the current backlog.
    fn control(&self, backlog: usize, need: usize) {
        let mut target = self.target.load(Ordering::Relaxed) as usize;
        self.min_backlog.set(self.min_backlog.get().min(backlog));
        self.window_blocks.set(self.window_blocks.get() + 1);

        if backlog < need {
            target = (target + TARGET_GROW).min(TARGET_MAX);
            self.target.store(target as u32, Ordering::Relaxed);
            self.min_backlog.set(usize::MAX);
            self.window_blocks.set(0);
        } else if self.window_blocks.get() >= CALM_WINDOW {
            if self.min_backlog.get() > target + need {
                target = target.saturating_sub(TARGET_SHRINK).max(TARGET_MIN);
                self.target.store(target as u32, Ordering::Relaxed);
            }
            self.min_backlog.set(usize::MAX);
            self.window_blocks.set(0);
        }

        // Drive the ratio from a smoothed backlog so it tracks real drift (slow)
        // and ignores per-block fill ripple (fast). A jump far beyond jitter
        // scale is a re-prime refill, not drift -- restart the EMA there so the
        // controller doesn't chase the refill as a huge error.
        let prev = self.avg_backlog.get();
        let avg = if prev < 0.0 || (backlog as f64 - prev).abs() > TARGET_GROW as f64 {
            backlog as f64
        } else {
            prev + DRIFT_BACKLOG_ALPHA * (backlog as f64 - prev)
        };
        self.avg_backlog.set(avg);

        let e = avg - target as f64;
        let e = if e.abs() < DRIFT_DEADBAND { 0.0 } else { e - DRIFT_DEADBAND.copysign(e) };
        let d = (1.0 - DRIFT_KP * e).clamp(DRIFT_MIN, DRIFT_MAX);
        self.drift.store((d as f32).to_bits(), Ordering::Relaxed);
    }

    /// Whether at least one channel has enough buffered to produce a block.
    /// Availability-paced outputs (file recording) use this to run at the
    /// network arrival rate.
    pub fn ready(&self, _block_len: usize) -> bool {
        match self.taps.try_lock() {
            Ok(taps) => taps.values().any(|t| t.backlog() >= t.need_in()),
            Err(_) => false,
        }
    }

    /// Copy one channel's already-resampled scratch into `out`.
    pub fn channel(&self, key: &str, out: &mut [f32]) {
        out.fill(0.0);
        let Ok(taps) = self.taps.try_lock() else { return };
        let Some(tap) = taps.get(key) else { return };
        let src = &tap.scratch[..tap.valid];
        // Taps are mono; a wider destination gets the channel centred across it.
        let width = out.len() / OUT_BLOCK_FRAMES;
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
            let width = (out.len() / OUT_BLOCK_FRAMES).max(1);
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
