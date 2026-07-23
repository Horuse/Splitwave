//! Lock-free SPSC queue carrying parameter changes from the UI thread to the
//! plugin's audio processor on the DSP worker. One `Arc<ParamRing>` is shared
//! by both ends and persists across graph rebuilds (kept in the effect
//! registry), so a rebuilt `PluginNode` keeps receiving the UI's writes.

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// Power of two so index wrap is a mask.
const CAPACITY: usize = 512;

struct Slot {
    id: AtomicU32,
    value: AtomicU64,
}

pub struct ParamRing {
    slots: Box<[Slot]>,
    // Consumer index (DSP worker).
    head: AtomicUsize,
    // Producer index (UI thread).
    tail: AtomicUsize,
    mask: usize,
}

impl ParamRing {
    pub fn new() -> Self {
        let slots = (0..CAPACITY)
            .map(|_| Slot {
                id: AtomicU32::new(0),
                value: AtomicU64::new(0),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            mask: CAPACITY - 1,
        }
    }

    /// UI thread. Drops the write when full (bounded by a slow worker); the UI
    /// re-sends the current value on the next knob move.
    pub fn push(&self, id: u32, value: f64) {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) >= self.slots.len() {
            return;
        }
        let slot = &self.slots[tail & self.mask];
        slot.id.store(id, Ordering::Relaxed);
        slot.value.store(value.to_bits(), Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
    }

    /// DSP worker. Returns the next `(param_id, value)` or `None` when empty.
    pub fn pop(&self) -> Option<(u32, f64)> {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let slot = &self.slots[head & self.mask];
        let id = slot.id.load(Ordering::Relaxed);
        let value = f64::from_bits(slot.value.load(Ordering::Relaxed));
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some((id, value))
    }
}
