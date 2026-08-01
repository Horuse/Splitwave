//! Dynamic Producer fan-out for live input streams.
//!
//! A cpal or SCK input callback needs to broadcast each block to N
//! subscriber rings -- one per output that consumes this input. The
//! subscriber set can change at runtime (reconcile adds/removes outputs)
//! and the callback runs on the RT thread, so we can't lock or allocate.
//!
//! `BroadcastTx` (main thread) sends add/remove commands over an SPSC
//! `rtrb` queue to `BroadcastRx` (RT thread). The RT side holds a
//! fixed-capacity `Vec<Option<Producer>>` and drains pending commands at
//! the top of each callback before broadcasting samples to active slots.
//!
//! Drop-ordering: removed `Producer`s are returned to the main thread via
//! a `discarded` SPSC queue rather than dropped on the RT thread. This
//! avoids the case where, after the matching `Consumer` was already
//! dropped on main, the `Producer::drop` on RT would call into the global
//! allocator to free the ring buffer. `BroadcastTx::drain_discarded`
//! collects the returned producers and drops them on main.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::audio::health;
use crate::audio::streams::bulk_push_counted;
use crate::error::{AppError, AppResult};

/// Maximum subscribers per input. 32 covers any plausible pipeline (each
/// output contributes one bridge per input it consumes); pre-allocated so
/// the RT side never grows its slot vector.
pub const BRIDGE_CAPACITY: usize = 32;

/// Headroom for in-flight commands. Reconcile bursts can issue N adds + N
/// removes back-to-back; 4x capacity keeps the cmd queue from saturating.
const CMD_QUEUE_CAPACITY: usize = BRIDGE_CAPACITY * 4;

/// Per-subscriber capture-side counters for one broadcast slot. Handed back
/// to the pipeline at `add()` time so it can pair a slot with the SourceMeta
/// of the DSP source reading the other end of the same ring -- the global
/// `health::CAPTURE_RING_OVERRUN_SAMPLES` total can't tell which input ring
/// is the one overflowing.
#[derive(Clone)]
pub struct CaptureStats {
    /// Samples successfully written to this slot's ring.
    pub fed: Arc<AtomicU64>,
    /// Samples offered but dropped because the ring was full.
    pub dropped: Arc<AtomicU64>,
}

impl CaptureStats {
    fn new() -> Self {
        Self {
            fed: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// One broadcast subscriber: the ring's Producer end plus its capture stats.
type Slot = (Producer<f32>, CaptureStats);

enum BroadcastCmd {
    Add { slot: usize, producer: Producer<f32>, stats: CaptureStats },
    Remove { slot: usize },
}

/// Main-thread side. Tracks slot allocations and pushes Add/Remove
/// commands to the RT callback.
pub struct BroadcastTx {
    cmds: Producer<BroadcastCmd>,
    /// `true` for slots currently bound to a Producer. Used by `add` to
    /// pick a free slot; RT side never reads this.
    used: Vec<bool>,
    /// RT returns removed producers here so they drop on main, not on the
    /// audio callback thread.
    discarded_rx: Consumer<Slot>,
}

/// RT-thread side. Owns the Producer slot vec; lives inside the input
/// callback closure.
pub struct BroadcastRx {
    cmds: Consumer<BroadcastCmd>,
    slots: Vec<Option<Slot>>,
    discarded_tx: Producer<Slot>,
}

pub fn broadcast_channel() -> (BroadcastTx, BroadcastRx) {
    let (cmd_tx, cmd_rx) = RingBuffer::<BroadcastCmd>::new(CMD_QUEUE_CAPACITY);
    let (disc_tx, disc_rx) = RingBuffer::<Slot>::new(CMD_QUEUE_CAPACITY);
    let mut slots = Vec::with_capacity(BRIDGE_CAPACITY);
    let mut used = Vec::with_capacity(BRIDGE_CAPACITY);
    for _ in 0..BRIDGE_CAPACITY {
        slots.push(None);
        used.push(false);
    }
    (
        BroadcastTx {
            cmds: cmd_tx,
            used,
            discarded_rx: disc_rx,
        },
        BroadcastRx {
            cmds: cmd_rx,
            slots,
            discarded_tx: disc_tx,
        },
    )
}

impl BroadcastTx {
    /// Register `producer` for broadcast. Returns the slot index used to
    /// remove it later, plus that slot's capture-side counters -- the
    /// caller hands these to the matching SourceMeta so the tick thread can
    /// log fed/dropped alongside the consumed side of the same ring.
    /// Errors if all slots are taken or the cmd queue is momentarily full
    /// (caller should retry after a reconcile cycle).
    pub fn add(&mut self, producer: Producer<f32>) -> AppResult<(usize, CaptureStats)> {
        let slot = self
            .used
            .iter()
            .position(|&b| !b)
            .ok_or_else(|| AppError::Validation("input broadcast slots exhausted".into()))?;
        let stats = CaptureStats::new();
        self.cmds
            .push(BroadcastCmd::Add { slot, producer, stats: stats.clone() })
            .map_err(|_| AppError::Stream("input broadcast cmd queue full".into()))?;
        self.used[slot] = true;
        Ok((slot, stats))
    }

    /// Slots still available for `add`.
    pub fn free_slots(&self) -> usize {
        self.used.iter().filter(|&&b| !b).count()
    }

    /// Unregister the producer at `slot`. Idempotent -- quietly no-ops if
    /// the slot was already free.
    pub fn remove(&mut self, slot: usize) -> AppResult<()> {
        if slot >= self.used.len() || !self.used[slot] {
            return Ok(());
        }
        self.cmds
            .push(BroadcastCmd::Remove { slot })
            .map_err(|_| AppError::Stream("input broadcast cmd queue full".into()))?;
        self.used[slot] = false;
        Ok(())
    }

    /// Collect and drop any producers the RT side returned via the
    /// discarded channel. Call after issuing Remove commands and before
    /// dropping the consumer side, so any pending allocator work happens
    /// on main rather than RT.
    pub fn drain_discarded(&mut self) {
        while self.discarded_rx.pop().is_ok() {}
    }
}

impl BroadcastRx {
    /// Drain pending commands. Call at the top of each audio callback.
    /// RT-safe -- bounded by `CMD_QUEUE_CAPACITY` per call, no alloc.
    #[inline]
    pub fn apply_commands(&mut self) {
        while let Ok(cmd) = self.cmds.pop() {
            match cmd {
                BroadcastCmd::Add { slot, producer, stats } => {
                    // If slot already had a Producer, return it to main
                    // before overwriting (defensive -- `BroadcastTx::add`
                    // only picks free slots, so this branch is rare).
                    if let Some(prev) = self.slots[slot].take() {
                        let _ = self.discarded_tx.push(prev);
                    }
                    self.slots[slot] = Some((producer, stats));
                }
                BroadcastCmd::Remove { slot } => {
                    if let Some(p) = self.slots[slot].take() {
                        let _ = self.discarded_tx.push(p);
                    }
                }
            }
        }
    }

    /// Broadcast `samples` to every active slot. RT-safe -- `bulk_push_counted`
    /// reserves via one CAS per slot and never blocks.
    #[inline]
    pub fn broadcast(&mut self, samples: &[f32]) {
        for slot in self.slots.iter_mut() {
            if let Some((p, stats)) = slot {
                let written =
                    bulk_push_counted(p, samples, &health::CAPTURE_RING_OVERRUN_SAMPLES);
                stats.fed.fetch_add(written as u64, Ordering::Relaxed);
                let dropped = (samples.len() - written) as u64;
                if dropped > 0 {
                    stats.dropped.fetch_add(dropped, Ordering::Relaxed);
                }
            }
        }
    }

    /// Samples queued in the fullest active slot, or `None` when nothing is
    /// subscribed. A file-driven source paces itself off this so its rate is
    /// dictated by the consumers rather than by a wall clock of its own.
    pub fn max_queued(&mut self) -> Option<usize> {
        self.apply_commands();
        self.slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|(p, _)| !p.is_abandoned())
            .map(|(p, _)| p.buffer().capacity() - p.slots())
            .max()
    }

    /// Push `samples` to every active slot without dropping on overflow --
    /// sleeps `backoff` and retries until each consumer drains enough room.
    /// NOT RT-safe; for offline / file-driven inputs where the consumer
    /// pace dictates source throughput.
    pub fn broadcast_blocking(
        &mut self,
        samples: &[f32],
        stop: &AtomicBool,
        paused: &AtomicBool,
        backoff: Duration,
    ) {
        self.apply_commands();
        for i in 0..self.slots.len() {
            let mut written = 0;
            while written < samples.len() {
                if stop.load(Ordering::SeqCst) || paused.load(Ordering::SeqCst) {
                    return;
                }
                let Some((p, _)) = self.slots[i].as_mut() else { break };
                // A consumer that went away leaves its ring full forever.
                if p.is_abandoned() {
                    break;
                }
                let avail = p.slots();
                if avail == 0 {
                    thread::sleep(backoff);
                    // The removal queued by a torn-down output only lands here.
                    self.apply_commands();
                    continue;
                }
                let take = avail.min(samples.len() - written);
                if let Ok(mut chunk) = p.write_chunk(take) {
                    let (first, second) = chunk.as_mut_slices();
                    let n1 = first.len();
                    first.copy_from_slice(&samples[written..written + n1]);
                    let n2 = second.len();
                    if n2 > 0 {
                        second.copy_from_slice(&samples[written + n1..written + n1 + n2]);
                    }
                    chunk.commit_all();
                    written += take;
                }
            }
            // Blocking push never drops (it waits for room), so only fed
            // needs updating here -- dropped stays at its initial 0.
            if let Some((_, stats)) = self.slots[i].as_ref() {
                stats.fed.fetch_add(written as u64, Ordering::Relaxed);
            }
        }
    }
}
