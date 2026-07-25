//! Lock-free broadcast (single-producer, multi-consumer) queue carrying
//! parameter changes from the UI thread to the plugin's audio processor(s) on
//! the DSP worker. One `Arc<ParamRing>` is shared by the UI end and every
//! `PluginNode` of the node, and persists across graph rebuilds (kept in the
//! effect registry), so a rebuilt `PluginNode` keeps receiving the UI's writes.
//!
//! A plugin wider than stereo runs one `PluginNode` per stereo pair, all on the
//! same worker thread. Each holds its own read cursor, so every pair applies
//! the same parameter change; a destructive SPSC queue would let the first pair
//! consume the write and starve the rest.
//!
//! The queue is lock-free for readers only. Writers take `producer`, because
//! there is more than one: the UI writes through `update_effect`, and a VST3
//! plugin's own editor writes through `IComponentHandler::performEdit` on the
//! main thread. Two unsynchronised writers would interleave a slot's payload
//! with its sequence number and hand the reader a torn event. The lock never
//! reaches the DSP worker, which only ever reads.

use std::sync::atomic::{fence, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

/// Power of two so index wrap is a mask.
const CAPACITY: usize = 512;

/// Slot sequence marking a write in progress; a reader that sees it treats the
/// slot as unavailable and stops.
const WRITING: usize = usize::MAX;

struct Slot {
    // The producer write index that last filled this slot, or `WRITING`.
    seq: AtomicUsize,
    id: AtomicU32,
    value: AtomicU64,
}

pub struct ParamRing {
    slots: Box<[Slot]>,
    // Total writes issued, monotonically increasing.
    tail: AtomicUsize,
    mask: usize,
    // Serialises writers. Never taken on the audio thread.
    producer: Mutex<()>,
}

impl ParamRing {
    pub fn new() -> Self {
        let slots = (0..CAPACITY)
            .map(|_| Slot {
                seq: AtomicUsize::new(WRITING),
                id: AtomicU32::new(0),
                value: AtomicU64::new(0),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            tail: AtomicUsize::new(0),
            mask: CAPACITY - 1,
            producer: Mutex::new(()),
        }
    }

    /// A consumer starts here so a freshly built `PluginNode` replays no writes
    /// issued before it existed.
    pub fn cursor(&self) -> usize {
        self.tail.load(Ordering::Acquire)
    }

    /// Any writer thread. Overwrites the oldest slot; a consumer lagging past
    /// capacity loses old writes, which the UI re-sends on the next knob move.
    pub fn push(&self, id: u32, value: f64) {
        let _writing = self.producer.lock().unwrap_or_else(|e| e.into_inner());
        let w = self.tail.load(Ordering::Relaxed);
        let slot = &self.slots[w & self.mask];
        slot.seq.store(WRITING, Ordering::Release);
        slot.id.store(id, Ordering::Relaxed);
        slot.value.store(value.to_bits(), Ordering::Relaxed);
        slot.seq.store(w, Ordering::Release);
        self.tail.store(w.wrapping_add(1), Ordering::Release);
    }

    /// DSP worker. Reads the next `(param_id, value)` at `cursor`, advancing it,
    /// or `None` when caught up. A slot overwritten mid-read is skipped as lost.
    pub fn read(&self, cursor: &mut usize) -> Option<(u32, f64)> {
        let tail = self.tail.load(Ordering::Acquire);
        // Bound catch-up work: a consumer behind by more than capacity jumps to
        // the oldest still-live write.
        if tail.wrapping_sub(*cursor) > self.slots.len() {
            *cursor = tail.wrapping_sub(self.slots.len());
        }
        while *cursor != tail {
            let c = *cursor;
            let slot = &self.slots[c & self.mask];
            let s1 = slot.seq.load(Ordering::Acquire);
            if s1 != c {
                // Overwritten by a newer write (or mid-write): this item is lost.
                *cursor = c.wrapping_add(1);
                continue;
            }
            let id = slot.id.load(Ordering::Relaxed);
            let value = slot.value.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            *cursor = c.wrapping_add(1);
            if slot.seq.load(Ordering::Acquire) == c {
                return Some((id, f64::from_bits(value)));
            }
        }
        None
    }
}
