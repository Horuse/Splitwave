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

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};

use rtrb::{Consumer, Producer, RingBuffer};

use crate::audio::resample::StereoResamplerOut;
use crate::audio::streams::bulk_push;

/// Decoded network audio is always carried at 48 kHz stereo.
pub const SR: u32 = 48_000;
/// Output frames produced per block; must equal the DSP worker's block size.
pub const OUT_BLOCK_FRAMES: usize = 1024;
/// Per-consumer, per-channel 48 kHz jitter ring (~1 s stereo) -- headroom for a
/// deep adaptive target plus a burst after a latency spike.
pub const CONSUMER_RING: usize = 96_000;

/// Adaptive jitter-buffer target (samples stereo at 48 kHz): the fill the drift
/// loop steers toward and the buffer primes to. Starts moderate, grows when the
/// network under-delivers (a latency spike drains it), shrinks slowly during
/// sustained calm -- so a periodic spike is absorbed after the first occurrence.
const TARGET_INIT: usize = 5_760; // ~60 ms
const TARGET_MIN: usize = 3_840; // ~40 ms
const TARGET_MAX: usize = 38_400; // ~400 ms
const TARGET_GROW: usize = 4_800; // +50 ms on underrun
const TARGET_SHRINK: usize = 960; // -10 ms per calm window
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

/// One received channel's fan-out: the 48 kHz ring producer for each live
/// consumer. The decode task pushes decoded audio into all of them.
pub type ChannelBroadcast = Arc<Mutex<Vec<Producer<f32>>>>;

/// A consumer's per-channel playback taps, keyed by channel id.
pub type TapMap = Arc<Mutex<HashMap<String, PlaybackTap>>>;

/// Handle returned when registering an output consumer.
pub struct ConsumerHandle {
    pub taps: TapMap,
    pub drift: Arc<AtomicU32>,
    pub target: Arc<AtomicU32>,
    pub realtime: bool,
}

/// Push decoded 48 kHz audio into every live consumer ring for a channel.
pub fn broadcast_push(broadcast: &ChannelBroadcast, samples: &[f32]) {
    let mut prods = broadcast.lock().unwrap();
    prods.retain(|p| !p.is_abandoned());
    for p in prods.iter_mut() {
        bulk_push(p, samples);
    }
}

/// One channel's playback state for one consumer: a 48 kHz jitter ring plus a
/// fixed-output resampler (48 kHz -> consumer rate) whose ratio tracks drift.
pub struct PlaybackTap {
    consumer: Consumer<f32>,
    resampler: StereoResamplerOut,
    base_ratio: f64,
    last_ratio: f64,
    realtime: bool,
    drift: Arc<AtomicU32>,
    target: Arc<AtomicU32>,
    in_buf: Vec<f32>,
    pub scratch: Vec<f32>,
    pub valid: usize,
    primed: bool,
    // PLC: the last real output block, and whether we're currently in a gap.
    // On a network underrun we fade this out (instead of a hard silence step),
    // and fade the real audio back in on recovery.
    last_block: Vec<f32>,
    gap: bool,
}

impl PlaybackTap {
    fn new(
        consumer: Consumer<f32>,
        rate: u32,
        realtime: bool,
        drift: Arc<AtomicU32>,
        target: Arc<AtomicU32>,
    ) -> Self {
        let base_ratio = rate as f64 / SR as f64;
        let resampler = StereoResamplerOut::new(SR, rate, OUT_BLOCK_FRAMES)
            .expect("stereo resampler init");
        Self {
            consumer,
            resampler,
            base_ratio,
            last_ratio: base_ratio,
            realtime,
            drift,
            target,
            in_buf: Vec::with_capacity(4096),
            scratch: Vec::with_capacity(OUT_BLOCK_FRAMES * 2),
            valid: 0,
            primed: false,
            last_block: vec![0.0; OUT_BLOCK_FRAMES * 2],
            gap: false,
        }
    }

    fn backlog(&self) -> usize {
        self.consumer.slots()
    }

    /// Input samples the next resample will consume (reflects the current ratio).
    fn need_in(&self) -> usize {
        self.resampler.input_frames_next() * 2
    }

    /// Produce one output block into `scratch` (valid = its length), resampling
    /// 48 kHz -> consumer rate at the drift-adjusted ratio. Returns the sample
    /// count (0 = emit silence: priming, or a real network underrun).
    fn fill_block(&mut self, _out_frames: usize) -> usize {
        // Track drift ratio for this block.
        if self.realtime {
            let d = f32::from_bits(self.drift.load(Ordering::Relaxed)) as f64;
            let ratio = self.base_ratio * d;
            if (ratio - self.last_ratio).abs() > 1e-9 {
                self.resampler.set_ratio(ratio);
                self.last_ratio = ratio;
            }
        }
        let target = self.target.load(Ordering::Relaxed) as usize;
        let need = self.need_in();

        if !self.primed {
            if self.consumer.slots() < target.max(need) {
                self.valid = 0;
                return 0;
            }
            self.primed = true;
        }

        // Safety net only (abnormal burst): the drift loop normally keeps the
        // ring near target. Generous headroom -- sender catch-up bursts after a
        // scheduler stall are legitimate and must not get spliced. Even sample
        // count so channels stay aligned.
        let hard_cap = (target * 2).max(target + OUT_BLOCK_FRAMES * 20);
        if self.consumer.slots() > hard_cap {
            let drop = (self.consumer.slots() - target) & !1;
            if let Ok(chunk) = self.consumer.read_chunk(drop) {
                chunk.commit_all();
                // The discard is a splice; fade back in over the next block.
                self.gap = true;
            }
        }

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
            let frames = self.scratch.len() / 2;
            for (i, fr) in self.scratch.chunks_mut(2).enumerate() {
                let g = (i as f32 + 1.0) / frames as f32;
                fr[0] *= g;
                fr[1] *= g;
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
        let frames = self.scratch.len() / 2;
        for (i, fr) in self.scratch.chunks_mut(2).enumerate() {
            let g = 1.0 - (i as f32 + 1.0) / frames as f32;
            fr[0] *= g;
            fr[1] *= g;
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
        }
    }
}

impl FanoutRegistry {
    /// New output consumer: an empty tap map wired a fresh ring into every known
    /// channel's broadcast. Locks `consumers` before `broadcasts`.
    pub fn register_consumer(&self, output_sr: u32, realtime: bool) -> ConsumerHandle {
        let map: TapMap = Arc::new(Mutex::new(HashMap::new()));
        let drift = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let target = Arc::new(AtomicU32::new(TARGET_INIT as u32));
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|c| c.taps.strong_count() > 0);
        for (key, bc) in self.broadcasts.lock().unwrap().iter() {
            let (prod, cons) = RingBuffer::<f32>::new(CONSUMER_RING);
            bc.lock().unwrap().push(prod);
            map.lock().unwrap().insert(
                key.clone(),
                PlaybackTap::new(cons, output_sr, realtime, drift.clone(), target.clone()),
            );
        }
        consumers.push(ConsumerRef {
            rate: output_sr,
            realtime,
            drift: drift.clone(),
            target: target.clone(),
            taps: Arc::downgrade(&map),
        });
        ConsumerHandle { taps: map, drift, target, realtime }
    }

    /// New received channel: a fresh broadcast wired into every live consumer.
    pub fn attach_channel(&self, key: String) -> ChannelBroadcast {
        let bc: ChannelBroadcast = Arc::new(Mutex::new(Vec::new()));
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|c| c.taps.strong_count() > 0);
        for c in consumers.iter() {
            if let Some(map) = c.taps.upgrade() {
                let (prod, cons) = RingBuffer::<f32>::new(CONSUMER_RING);
                bc.lock().unwrap().push(prod);
                map.lock().unwrap().insert(
                    key.clone(),
                    PlaybackTap::new(cons, c.rate, c.realtime, c.drift.clone(), c.target.clone()),
                );
            }
        }
        self.broadcasts.lock().unwrap().insert(key, bc.clone());
        bc
    }

    /// Drops channels whose key starts with `prefix`.
    pub fn drop_prefix(&self, prefix: &str) {
        self.broadcasts.lock().unwrap().retain(|k, _| !k.starts_with(prefix));
    }

    pub fn clear(&self) {
        self.broadcasts.lock().unwrap().clear();
        self.consumers.lock().unwrap().clear();
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
        }
    }

    /// Resample one block from every tap into its `scratch` and sum into `mix`.
    /// Real-time consumers also adapt the buffer depth and drift ratio here.
    pub fn mix_block(&self, mix: &mut [f32]) {
        let out_frames = mix.len() / 2;
        // The playback resamplers are built for exactly OUT_BLOCK_FRAMES output.
        debug_assert_eq!(out_frames, OUT_BLOCK_FRAMES);
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
        if self.realtime {
            // Drive from the emptiest channel so no channel is left to underrun;
            // one shared ratio keeps channels phase-coherent.
            let mut min_backlog = usize::MAX;
            let mut need = OUT_BLOCK_FRAMES * 2;
            for tap in taps.values() {
                min_backlog = min_backlog.min(tap.backlog());
                need = tap.need_in();
            }
            if min_backlog != usize::MAX {
                self.control(min_backlog, need);
            }
        }
        for tap in taps.values_mut() {
            let n = tap.fill_block(out_frames);
            for (d, &v) in mix[..n].iter_mut().zip(tap.scratch[..n].iter()) {
                *d += v;
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
        if let Ok(taps) = self.taps.try_lock() {
            if let Some(tap) = taps.get(key) {
                let n = tap.valid.min(out.len());
                out[..n].copy_from_slice(&tap.scratch[..n]);
            }
        }
    }

    /// Sum every channel whose key starts with `prefix` into `out`.
    pub fn prefix_mix(&self, prefix: &str, out: &mut [f32]) {
        out.fill(0.0);
        if let Ok(taps) = self.taps.try_lock() {
            for (key, tap) in taps.iter() {
                if key.starts_with(prefix) {
                    let n = tap.valid.min(out.len());
                    for (d, &v) in out[..n].iter_mut().zip(tap.scratch[..n].iter()) {
                        *d += v;
                    }
                }
            }
        }
    }
}
