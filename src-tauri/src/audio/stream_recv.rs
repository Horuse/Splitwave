//! Shared receive path for network audio (WebRTC and direct-IP). Decoded audio
//! arrives at 48 kHz on a ring; a fan-out task hands it to every live consumer,
//! resampling to each consumer's graph rate. Every output subgraph gets its own
//! ring, so audio is never drained twice.
//!
//! Clock-drift compensation: a real-time consumer (speaker) runs on a device
//! clock that drifts against the sender's 48 kHz clock. A proportional
//! controller watches that consumer's jitter-buffer fill and nudges the resample
//! ratio by a fraction of a percent so playback tracks the sender continuously,
//! without ever dropping or inserting samples (which would be audible).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::audio::resample::StereoResampler;
use crate::audio::streams::bulk_push;

/// Decoded network audio is always carried at 48 kHz stereo.
pub const SR: u32 = 48_000;
pub const RESAMPLE_CHUNK: usize = 256;
/// 48 kHz decoded ring feeding the fan-out task (~0.5 s stereo).
pub const RECV_RING: usize = 48_000;
/// Output-rate jitter buffer one consumer drains a block at a time.
pub const PLAYBACK_RING: usize = 48_000;
/// Max samples a consumer pulls per block (DSP_BLOCK_FRAMES * 2 channels).
pub const PLAYBACK_SCRATCH: usize = 2048;
/// Buffer this many samples (~50 ms stereo) before playback starts, and
/// re-buffer after a full drain, so network jitter doesn't stutter output.
pub const PLAYBACK_PRIME: usize = 4800;
/// Backlog the drift controller steers toward (~100 ms stereo).
pub const PLAYBACK_TARGET: usize = 9600;
/// Above this backlog (~200 ms) something abnormal happened (burst, resume);
/// skip ahead to the target as a safety net. Normal drift never reaches it --
/// the resampler controller keeps the backlog near PLAYBACK_TARGET.
pub const PLAYBACK_MAX: usize = 19_200;

/// Proportional gain: backlog error (samples) -> ratio correction. Sized so a
/// steady drift settles with only a few ms of extra backlog, and the ratio
/// stays well inside DRIFT_MIN/MAX for any realistic clock mismatch.
const DRIFT_KP: f64 = 2.0e-6;
const DRIFT_MIN: f64 = 0.97;
const DRIFT_MAX: f64 = 1.03;

/// One received channel fans out to every consumer via a list of `Target`s;
/// the fan-out task resamples the 48 kHz stream into each.
pub type ChannelBroadcast = Arc<Mutex<Vec<Target>>>;

/// A consumer's per-channel jitter-buffered rings, keyed by channel id.
pub type TapMap = Arc<Mutex<HashMap<String, PlaybackTap>>>;

/// Handle returned when registering an output consumer: its per-channel tap map
/// plus the shared drift cell the fan-out reads and the (real-time only)
/// controller writes.
pub struct ConsumerHandle {
    pub taps: TapMap,
    pub drift: Arc<AtomicU32>,
    pub realtime: bool,
}

/// One fan-out destination: a single consumer's ring for one channel, with its
/// own resampler (48 kHz -> that consumer's rate) whose ratio is `base * drift`.
/// A `None` resampler is an identity path (matched rate, non-real-time consumer
/// that needs no drift tracking), kept sample-exact.
pub struct Target {
    prod: Producer<f32>,
    resampler: Option<StereoResampler>,
    base_ratio: f64,
    last_ratio: f64,
    drift: Arc<AtomicU32>,
    in_acc: Vec<f32>,
    out_acc: Vec<f32>,
}

impl Target {
    fn new(rate: u32, realtime: bool, prod: Producer<f32>, drift: Arc<AtomicU32>) -> Self {
        let base_ratio = rate as f64 / SR as f64;
        // Matched-rate + no drift tracking => identity (bit-exact, e.g. file
        // capture). Otherwise a resampler, even at 1.0, so drift can nudge it.
        let resampler = if rate == SR && !realtime {
            None
        } else {
            StereoResampler::new(SR, rate, RESAMPLE_CHUNK).ok()
        };
        Self {
            prod,
            resampler,
            base_ratio,
            last_ratio: base_ratio,
            drift,
            in_acc: Vec::new(),
            out_acc: Vec::new(),
        }
    }

    fn feed(&mut self, samples: &[f32]) {
        let Some(r) = self.resampler.as_mut() else {
            bulk_push(&mut self.prod, samples);
            return;
        };
        let d = f32::from_bits(self.drift.load(Ordering::Relaxed)) as f64;
        let ratio = self.base_ratio * d;
        if (ratio - self.last_ratio).abs() > 1e-9 {
            r.set_ratio(ratio);
            self.last_ratio = ratio;
        }
        self.in_acc.extend_from_slice(samples);
        self.out_acc.clear();
        let need = r.chunk_in() * 2;
        let mut off = 0;
        while self.in_acc.len() - off >= need {
            if r.process_chunk(&self.in_acc[off..off + need], &mut self.out_acc).is_err() {
                break;
            }
            off += need;
        }
        self.in_acc.drain(..off);
        if !self.out_acc.is_empty() {
            bulk_push(&mut self.prod, &self.out_acc);
        }
    }
}

/// RT-side jitter buffer for one channel. The fan-out task pushes resampled,
/// output-rate audio into the ring; the reader pops one block per callback into
/// `scratch` (read by both a mix and any per-channel output).
pub struct PlaybackTap {
    consumer: Consumer<f32>,
    pub scratch: Vec<f32>,
    pub valid: usize,
    primed: bool,
}

impl PlaybackTap {
    pub fn new(consumer: Consumer<f32>) -> Self {
        Self {
            consumer,
            scratch: vec![0.0; PLAYBACK_SCRATCH],
            valid: 0,
            primed: false,
        }
    }

    fn backlog(&self) -> usize {
        self.consumer.slots()
    }

    /// Pop up to `block_len` samples into `scratch`, applying the jitter buffer.
    /// Returns the count written; the rest of `scratch` is stale, so callers
    /// must read only `..valid`.
    pub fn fill_block(&mut self, block_len: usize) -> usize {
        let need = block_len.min(self.scratch.len());
        let mut avail = self.consumer.slots();
        if !self.primed {
            if avail < PLAYBACK_PRIME {
                self.valid = 0;
                return 0;
            }
            self.primed = true;
        }
        if avail == 0 {
            self.primed = false;
            self.valid = 0;
            return 0;
        }
        // Safety net only: normal drift is absorbed by the resampler controller,
        // so this fires only on an abnormal burst/resume. Even sample count so
        // channels stay aligned.
        if avail > PLAYBACK_MAX {
            let drop = (avail - PLAYBACK_TARGET) & !1;
            if let Ok(chunk) = self.consumer.read_chunk(drop) {
                chunk.commit_all();
            }
            avail = self.consumer.slots();
        }
        let n = need.min(avail);
        if let Ok(chunk) = self.consumer.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            self.scratch[..a.len()].copy_from_slice(a);
            self.scratch[a.len()..a.len() + b.len()].copy_from_slice(b);
            chunk.commit_all();
        }
        self.valid = n;
        n
    }
}

/// Fans received channels out to every output consumer. Each received channel
/// gets a `ChannelBroadcast`; each output subgraph registers a `TapMap` wired a
/// fresh ring per channel, so audio is never drained twice. Shared by the
/// WebRTC bridge and the direct-IP receiver.
pub struct FanoutRegistry {
    broadcasts: Mutex<HashMap<String, ChannelBroadcast>>,
    consumers: Mutex<Vec<ConsumerRef>>,
}

struct ConsumerRef {
    rate: u32,
    realtime: bool,
    drift: Arc<AtomicU32>,
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
    /// New output consumer: an empty tap map wired into every known channel's
    /// broadcast, tracked weakly so later channels attach to it too. Locks
    /// `consumers` before `broadcasts` (see `attach_channel`).
    pub fn register_consumer(&self, output_sr: u32, realtime: bool) -> ConsumerHandle {
        let map: TapMap = Arc::new(Mutex::new(HashMap::new()));
        let drift = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|c| c.taps.strong_count() > 0);
        for (key, bc) in self.broadcasts.lock().unwrap().iter() {
            let (prod, cons) = RingBuffer::<f32>::new(PLAYBACK_RING);
            bc.lock().unwrap().push(Target::new(output_sr, realtime, prod, drift.clone()));
            map.lock().unwrap().insert(key.clone(), PlaybackTap::new(cons));
        }
        consumers.push(ConsumerRef {
            rate: output_sr,
            realtime,
            drift: drift.clone(),
            taps: Arc::downgrade(&map),
        });
        ConsumerHandle { taps: map, drift, realtime }
    }

    /// New received channel: a fresh broadcast wired into every live consumer.
    pub fn attach_channel(&self, key: String) -> ChannelBroadcast {
        let bc: ChannelBroadcast = Arc::new(Mutex::new(Vec::new()));
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|c| c.taps.strong_count() > 0);
        for c in consumers.iter() {
            if let Some(map) = c.taps.upgrade() {
                let (prod, cons) = RingBuffer::<f32>::new(PLAYBACK_RING);
                bc.lock().unwrap().push(Target::new(c.rate, c.realtime, prod, c.drift.clone()));
                map.lock().unwrap().insert(key.clone(), PlaybackTap::new(cons));
            }
        }
        self.broadcasts.lock().unwrap().insert(key, bc.clone());
        bc
    }

    /// Drops channels whose key starts with `prefix` so new consumers don't wire
    /// to a gone source.
    pub fn drop_prefix(&self, prefix: &str) {
        self.broadcasts.lock().unwrap().retain(|k, _| !k.starts_with(prefix));
    }

    pub fn clear(&self) {
        self.broadcasts.lock().unwrap().clear();
        self.consumers.lock().unwrap().clear();
    }
}

/// RT-side reader over a channel tap map, shared by every node that emits
/// received audio (WebRTC bridge, direct-IP receiver). Owns the jitter pop +
/// summing and, for a real-time consumer, the drift controller.
pub struct ChannelReceiver {
    taps: TapMap,
    drift: Arc<AtomicU32>,
    realtime: bool,
}

impl ChannelReceiver {
    pub fn new(handle: ConsumerHandle) -> Self {
        Self {
            taps: handle.taps,
            drift: handle.drift,
            realtime: handle.realtime,
        }
    }

    /// Pop one block from every tap into its `scratch` and sum into `mix`. Call
    /// once per block before any `channel`/`prefix_mix` reads (they reuse the
    /// popped scratch). Real-time consumers also update the drift ratio here
    /// from the current jitter-buffer fill.
    pub fn mix_block(&self, mix: &mut [f32]) {
        mix.fill(0.0);
        if let Ok(mut taps) = self.taps.try_lock() {
            if self.realtime {
                // All channels of a consumer drain in lockstep, so any tap's
                // backlog represents this consumer; one shared ratio keeps the
                // channels phase-coherent.
                if let Some(tap) = taps.values().next() {
                    let e = tap.backlog() as f64 - PLAYBACK_TARGET as f64;
                    let d = (1.0 - DRIFT_KP * e).clamp(DRIFT_MIN, DRIFT_MAX);
                    self.drift.store((d as f32).to_bits(), Ordering::Relaxed);
                }
            }
            for tap in taps.values_mut() {
                let n = tap.fill_block(mix.len());
                for (d, &v) in mix[..n].iter_mut().zip(tap.scratch[..n].iter()) {
                    *d += v;
                }
            }
        }
    }

    /// Whether at least one channel has a full block buffered. Availability-paced
    /// outputs (file recording) use this to run at the network arrival rate
    /// instead of spinning flat-out when no captured source gates them.
    pub fn ready(&self, block_len: usize) -> bool {
        match self.taps.try_lock() {
            Ok(taps) => taps.values().any(|t| t.backlog() >= block_len),
            Err(_) => false,
        }
    }

    /// Copy one channel's already-popped scratch into `out`.
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

/// Drains a channel's 48 kHz receive ring and hands it to every live `Target`,
/// which resamples (with per-consumer drift) into that consumer's ring.
pub fn spawn_recv_fanout_task(mut consumer: Consumer<f32>, broadcast: ChannelBroadcast) {
    tauri::async_runtime::spawn(async move {
        let mut new: Vec<f32> = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            // The source was dropped (disconnect / room cancelled) -- exit.
            if consumer.is_abandoned() {
                return;
            }

            new.clear();
            let avail = consumer.slots();
            if avail > 0 {
                if let Ok(chunk) = consumer.read_chunk(avail) {
                    let (a, b) = chunk.as_slices();
                    new.extend_from_slice(a);
                    new.extend_from_slice(b);
                    chunk.commit_all();
                }
            }

            let mut targets = broadcast.lock().unwrap();
            targets.retain(|t| !t.prod.is_abandoned());
            for t in targets.iter_mut() {
                t.feed(&new);
            }
        }
    });
}
