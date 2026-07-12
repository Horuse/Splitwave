//! Shared receive path for network audio (WebRTC and direct-IP). Decoded audio
//! arrives at 48 kHz on a ring; a fan-out task resamples it to each live
//! consumer's graph rate and pushes into that consumer's jitter buffer. Every
//! output subgraph gets its own ring, so audio is never drained twice.

use std::collections::HashMap;
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
/// Buffer this many samples (~40 ms stereo) before playback starts, and
/// re-buffer after a full drain, so network jitter doesn't stutter output.
pub const PLAYBACK_PRIME: usize = 4096;
/// Above this backlog (~120 ms) drift has piled up; skip ahead to bound latency.
pub const PLAYBACK_MAX: usize = 12_288;

/// One received channel fans out to every consumer: `(graph rate, ring
/// producer)` per live consumer. The fan-out task resamples 48 kHz to each
/// distinct rate.
pub type ChannelBroadcast = Arc<Mutex<Vec<(u32, Producer<f32>)>>>;

/// A consumer's per-channel jitter-buffered rings, keyed by channel id.
pub type TapMap = Arc<Mutex<HashMap<String, PlaybackTap>>>;

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
        // Drop drift backlog (even sample count so channels stay aligned).
        if avail > PLAYBACK_MAX {
            let drop = (avail - PLAYBACK_PRIME) & !1;
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
/// gets a `ChannelBroadcast`; each output subgraph registers a `TapMap` that is
/// wired a fresh ring per channel, so audio is never drained twice. Shared by
/// the WebRTC bridge and the direct-IP receiver.
pub struct FanoutRegistry {
    broadcasts: Mutex<HashMap<String, ChannelBroadcast>>,
    // (graph rate, weak ref to that consumer's tap map) per live output.
    consumers: Mutex<Vec<(u32, Weak<Mutex<HashMap<String, PlaybackTap>>>)>>,
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
    pub fn register_consumer(&self, output_sr: u32) -> TapMap {
        let map: TapMap = Arc::new(Mutex::new(HashMap::new()));
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|(_, w)| w.strong_count() > 0);
        for (key, bc) in self.broadcasts.lock().unwrap().iter() {
            let (prod, cons) = RingBuffer::<f32>::new(PLAYBACK_RING);
            bc.lock().unwrap().push((output_sr, prod));
            map.lock().unwrap().insert(key.clone(), PlaybackTap::new(cons));
        }
        consumers.push((output_sr, Arc::downgrade(&map)));
        map
    }

    /// New received channel: a fresh broadcast wired into every live consumer.
    pub fn attach_channel(&self, key: String) -> ChannelBroadcast {
        let bc: ChannelBroadcast = Arc::new(Mutex::new(Vec::new()));
        let mut consumers = self.consumers.lock().unwrap();
        consumers.retain(|(_, w)| w.strong_count() > 0);
        for (sr, w) in consumers.iter() {
            if let Some(map) = w.upgrade() {
                let (prod, cons) = RingBuffer::<f32>::new(PLAYBACK_RING);
                bc.lock().unwrap().push((*sr, prod));
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
/// received audio (WebRTC bridge, direct-IP receiver). Naming of channels is
/// left to the caller; this only owns the jitter pop + summing.
pub struct ChannelReceiver {
    taps: TapMap,
}

impl ChannelReceiver {
    pub fn new(taps: TapMap) -> Self {
        Self { taps }
    }

    /// Pop one block from every tap into its `scratch` and sum into `mix`. Call
    /// once per block before any `channel`/`prefix_mix` reads (they reuse the
    /// popped scratch).
    pub fn mix_block(&self, mix: &mut [f32]) {
        mix.fill(0.0);
        if let Ok(mut taps) = self.taps.try_lock() {
            for tap in taps.values_mut() {
                let n = tap.fill_block(mix.len());
                for (d, &v) in mix[..n].iter_mut().zip(tap.scratch[..n].iter()) {
                    *d += v;
                }
            }
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

/// Per-target-rate resample state; one per distinct consumer sample rate.
struct RateState {
    resampler: Option<StereoResampler>,
    in_acc: Vec<f32>,
    out_acc: Vec<f32>,
}

impl RateState {
    fn new(rate: u32) -> Self {
        let resampler = if rate == SR {
            None
        } else {
            StereoResampler::new(SR, rate, RESAMPLE_CHUNK).ok()
        };
        Self { resampler, in_acc: Vec::new(), out_acc: Vec::new() }
    }

    fn feed(&mut self, samples: &[f32]) {
        self.out_acc.clear();
        match self.resampler.as_mut() {
            Some(r) => {
                self.in_acc.extend_from_slice(samples);
                let need = r.chunk_in() * 2;
                let mut off = 0;
                while self.in_acc.len() - off >= need {
                    if r.process_chunk(&self.in_acc[off..off + need], &mut self.out_acc).is_err() {
                        break;
                    }
                    off += need;
                }
                self.in_acc.drain(..off);
            }
            None => self.out_acc.extend_from_slice(samples),
        }
    }
}

/// Drains a channel's 48 kHz receive ring and fans it into every live consumer
/// ring, resampling once per distinct consumer rate (a speaker output and a
/// monitor graph can run at different rates).
pub fn spawn_recv_fanout_task(mut consumer: Consumer<f32>, broadcast: ChannelBroadcast) {
    tauri::async_runtime::spawn(async move {
        let mut states: HashMap<u32, RateState> = HashMap::new();
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

            let mut producers = broadcast.lock().unwrap();
            producers.retain(|(_, p)| !p.is_abandoned());
            let rates: Vec<u32> = {
                let mut r: Vec<u32> = producers.iter().map(|(sr, _)| *sr).collect();
                r.sort_unstable();
                r.dedup();
                r
            };
            states.retain(|rate, _| rates.contains(rate));
            for rate in rates {
                states.entry(rate).or_insert_with(|| RateState::new(rate)).feed(&new);
            }
            for (sr, prod) in producers.iter_mut() {
                if let Some(st) = states.get(sr) {
                    if !st.out_acc.is_empty() {
                        bulk_push(prod, &st.out_acc);
                    }
                }
            }
        }
    });
}

