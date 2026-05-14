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

use rtrb::{Consumer, Producer, RingBuffer};

use crate::audio::streams::bulk_push;
use crate::error::{AppError, AppResult};

/// Maximum subscribers per input. 32 covers any plausible pipeline (each
/// output contributes one bridge per input it consumes); pre-allocated so
/// the RT side never grows its slot vector.
pub const BRIDGE_CAPACITY: usize = 32;

/// Headroom for in-flight commands. Reconcile bursts can issue N adds + N
/// removes back-to-back; 4x capacity keeps the cmd queue from saturating.
const CMD_QUEUE_CAPACITY: usize = BRIDGE_CAPACITY * 4;

enum BroadcastCmd {
    Add { slot: usize, producer: Producer<f32> },
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
    discarded_rx: Consumer<Producer<f32>>,
}

/// RT-thread side. Owns the Producer slot vec; lives inside the input
/// callback closure.
pub struct BroadcastRx {
    cmds: Consumer<BroadcastCmd>,
    slots: Vec<Option<Producer<f32>>>,
    discarded_tx: Producer<Producer<f32>>,
}

pub fn broadcast_channel() -> (BroadcastTx, BroadcastRx) {
    let (cmd_tx, cmd_rx) = RingBuffer::<BroadcastCmd>::new(CMD_QUEUE_CAPACITY);
    let (disc_tx, disc_rx) = RingBuffer::<Producer<f32>>::new(BRIDGE_CAPACITY);
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
    /// remove it later. Errors if all slots are taken or the cmd queue is
    /// momentarily full (caller should retry after a reconcile cycle).
    pub fn add(&mut self, producer: Producer<f32>) -> AppResult<usize> {
        let slot = self
            .used
            .iter()
            .position(|&b| !b)
            .ok_or_else(|| AppError::Validation("input broadcast slots exhausted".into()))?;
        self.cmds
            .push(BroadcastCmd::Add { slot, producer })
            .map_err(|_| AppError::Stream("input broadcast cmd queue full".into()))?;
        self.used[slot] = true;
        Ok(slot)
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
                BroadcastCmd::Add { slot, producer } => {
                    // If slot already had a Producer, return it to main
                    // before overwriting (defensive -- `BroadcastTx::add`
                    // only picks free slots, so this branch is rare).
                    if let Some(prev) = self.slots[slot].take() {
                        let _ = self.discarded_tx.push(prev);
                    }
                    self.slots[slot] = Some(producer);
                }
                BroadcastCmd::Remove { slot } => {
                    if let Some(p) = self.slots[slot].take() {
                        // `push` is fallible only when the discarded
                        // queue is full -- sized to `BRIDGE_CAPACITY`, so
                        // it can't overflow under normal use. As a last
                        // resort the producer drops on RT.
                        let _ = self.discarded_tx.push(p);
                    }
                }
            }
        }
    }

    /// Broadcast `samples` to every active slot. RT-safe -- `bulk_push`
    /// reserves via one CAS per slot and never blocks.
    #[inline]
    pub fn broadcast(&mut self, samples: &[f32]) {
        for slot in self.slots.iter_mut() {
            if let Some(p) = slot {
                bulk_push(p, samples);
            }
        }
    }
}
