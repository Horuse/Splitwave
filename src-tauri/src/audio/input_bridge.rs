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
use crate::audio::wake::Doorbell;
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
    Add {
        slot: usize,
        producer: Producer<f32>,
        stats: CaptureStats,
    },
    Remove {
        slot: usize,
    },
}

/// Seconds since a process-wide epoch. Producers and consumers of audio
/// stamp their timing on this one clock.
pub fn now_secs() -> f64 {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
}

/// When a producer last wrote and how much it has written in total. A
/// consumer reads the queue as a smooth function of time from it (see
/// `drift_loop::ArrivalClock`): the level alone saws with every delivery.
#[derive(Default)]
pub struct WriteClock {
    samples: AtomicU64,
    /// `now_secs()` of the last write, as f64 bits.
    at: AtomicU64,
}

impl WriteClock {
    /// RT-safe: two relaxed stores.
    #[inline]
    pub fn record(&self, samples: usize, at: f64) {
        self.at.store(at.to_bits(), Ordering::Relaxed);
        self.samples.fetch_add(samples as u64, Ordering::Release);
    }

    /// Total samples written and when the last of them was. The two are read
    /// separately; a torn pair is one block's glitch the consumer lowpasses.
    #[inline]
    pub fn read(&self) -> (u64, f64) {
        let samples = self.samples.load(Ordering::Acquire);
        (samples, f64::from_bits(self.at.load(Ordering::Relaxed)))
    }
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
    clock: Arc<WriteClock>,
}

/// RT-thread side. Owns the Producer slot vec; lives inside the input
/// callback closure.
pub struct BroadcastRx {
    cmds: Consumer<BroadcastCmd>,
    slots: Vec<Option<Slot>>,
    discarded_tx: Producer<Slot>,
    clock: Arc<WriteClock>,
    /// Rung after every broadcast, for a thread that waits on this bridge.
    bell: Option<Arc<Doorbell>>,
}

pub fn broadcast_channel() -> (BroadcastTx, BroadcastRx) {
    let clock = Arc::new(WriteClock::default());
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
            clock: clock.clone(),
        },
        BroadcastRx {
            cmds: cmd_rx,
            slots,
            discarded_tx: disc_tx,
            clock,
            bell: None,
        },
    )
}

impl BroadcastTx {
    /// The input's write timing, shared with every graph that reads it.
    pub fn write_clock(&self) -> Arc<WriteClock> {
        self.clock.clone()
    }

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
            .push(BroadcastCmd::Add {
                slot,
                producer,
                stats: stats.clone(),
            })
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
                BroadcastCmd::Add {
                    slot,
                    producer,
                    stats,
                } => {
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
        self.clock.record(samples.len(), now_secs());
        for slot in self.slots.iter_mut() {
            if let Some((p, stats)) = slot {
                let written = bulk_push_counted(p, samples, &health::CAPTURE_RING_OVERRUN_SAMPLES);
                stats.fed.fetch_add(written as u64, Ordering::Relaxed);
                let dropped = (samples.len() - written) as u64;
                if dropped > 0 {
                    stats.dropped.fetch_add(dropped, Ordering::Relaxed);
                }
            }
        }
        if let Some(bell) = &self.bell {
            bell.ring();
        }
    }

    /// Wakes whoever answers `bell` after every broadcast.
    pub fn ring_after_broadcast(&mut self, bell: Arc<Doorbell>) {
        self.bell = Some(bell);
    }

    /// Samples queued in the emptiest active slot: the reader furthest along,
    /// or `None` when nothing is subscribed.
    pub fn min_queued(&mut self) -> Option<usize> {
        self.apply_commands();
        self.slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|(p, _)| !p.is_abandoned())
            .map(|(p, _)| p.buffer().capacity() - p.slots())
            .min()
    }

    /// Subscribers still reading.
    pub fn active_consumers(&mut self) -> usize {
        self.apply_commands();
        self.slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|(p, _)| !p.is_abandoned())
            .count()
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
                let Some((p, _)) = self.slots[i].as_mut() else {
                    break;
                };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Drains up to `dst.len()` samples, zero-filling the rest.
    fn pop_into(cons: &mut Consumer<f32>, dst: &mut [f32]) -> usize {
        let n = dst.len().min(cons.slots());
        if let Ok(chunk) = cons.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            dst[..a.len()].copy_from_slice(a);
            dst[a.len()..n].copy_from_slice(b);
            chunk.commit_all();
        }
        dst[n..].fill(0.0);
        n
    }

    #[test]
    fn add_remove_roundtrip_bridges_audio() {
        let (mut tx, mut rx) = broadcast_channel();
        assert_eq!(tx.free_slots(), BRIDGE_CAPACITY);

        let (prod, mut cons) = RingBuffer::<f32>::new(4096);
        let (slot, stats) = tx.add(prod).expect("add");

        rx.apply_commands();
        // Audio broadcast before the consumer drains: fed counter grows.
        let block: Vec<f32> = (0..512).map(|i| i as f32 * 0.001).collect();
        rx.broadcast(&block);
        assert_eq!(stats.fed.load(Ordering::Relaxed), 512);
        assert_eq!(stats.dropped.load(Ordering::Relaxed), 0);
        let mut out = vec![0.0f32; 512];
        let n = pop_into(&mut cons, &mut out);
        assert_eq!(n, 512);
        assert_eq!(out, block);

        // Remove; the discarded producer returns to main for teardown.
        tx.remove(slot).expect("remove");
        rx.apply_commands();
        tx.drain_discarded();
        assert_eq!(tx.free_slots(), BRIDGE_CAPACITY, "slot freed again");
        // Broadcast after removal goes nowhere and must not panic.
        rx.broadcast(&block);
    }

    #[test]
    fn broadcast_counts_drops_when_the_ring_is_full() {
        let (mut tx, mut rx) = broadcast_channel();
        // Tiny ring: 8 samples total.
        let (prod, mut cons) = RingBuffer::<f32>::new(8);
        let (_, stats) = tx.add(prod).expect("add");
        rx.apply_commands();
        rx.broadcast(&vec![1.0f32; 6]);
        // Does not fit whole: dropped whole, so no frame is ever split.
        rx.broadcast(&vec![2.0f32; 4]);
        assert_eq!(stats.fed.load(Ordering::Relaxed), 6);
        assert_eq!(stats.dropped.load(Ordering::Relaxed), 4);
        let mut out = vec![0.0f32; 8];
        assert_eq!(pop_into(&mut cons, &mut out), 6);
        assert_eq!(out[..6], [1.0; 6]);
    }

    // A mono 48 kHz mic normalized to 44.1 kHz: the resampler emits 235 and
    // 236 frames in turn, and a mono frame is one sample, so half its chunks
    // are odd. Every sample it emits must reach the graph.
    #[test]
    fn a_resampled_mono_mic_loses_nothing() {
        let (mut tx, mut rx) = broadcast_channel();
        let (prod, cons) = RingBuffer::<f32>::new(96_000);
        let (_, stats) = tx.add(prod).expect("add");
        rx.apply_commands();
        let mut rs = crate::audio::resample::MultiResampler::new(48_000, 44_100, 256, 1).unwrap();
        let input = vec![0.25f32; 256];
        let mut out = Vec::with_capacity(rs.out_max());
        let (mut emitted, mut odd) = (0usize, 0usize);
        for _ in 0..48_000 / 256 {
            out.clear();
            rs.process_chunk(&input, &mut out).unwrap();
            odd += out.len() % 2;
            emitted += out.len();
            rx.broadcast(&out);
        }
        assert!(odd > 0, "the case under test: odd chunks happen");
        assert_eq!(stats.dropped.load(Ordering::Relaxed), 0);
        assert_eq!(cons.slots(), emitted, "every resampled sample arrived");
    }

    #[test]
    fn slots_exhaust_and_free_up() {
        let mut tx = broadcast_channel().0;
        for _ in 0..BRIDGE_CAPACITY {
            let (prod, _) = RingBuffer::<f32>::new(64);
            tx.add(prod).expect("slot free");
        }
        assert_eq!(tx.free_slots(), 0);
        let (prod, _) = RingBuffer::<f32>::new(64);
        let err = match tx.add(prod) {
            Ok(_) => panic!("no slots left"),
            Err(err) => err,
        };
        assert!(format!("{err}").contains("exhausted"));
        tx.remove(0).expect("remove");
        assert_eq!(tx.free_slots(), 1);
    }

    #[test]
    fn remove_is_idempotent_and_out_of_range_safe() {
        let mut tx = broadcast_channel().0;
        tx.remove(5).expect("no-op on free slot");
        tx.remove(usize::MAX).expect("out of range no-op");
    }

    #[test]
    fn max_queued_reports_the_fullest_slot() {
        let (mut tx, mut rx) = broadcast_channel();
        assert_eq!(rx.max_queued(), None, "nothing subscribed");

        let (prod, cons) = RingBuffer::<f32>::new(1024);
        let _keep_consumer = cons;
        let _ = tx.add(prod).expect("add");
        rx.apply_commands();
        // Empty ring: queued (filled) = capacity - free = 0.
        assert_eq!(rx.max_queued(), Some(0));
        // Abandoned consumer (dropped) is excluded.
        drop(_keep_consumer);
        rx.apply_commands();
        assert_eq!(rx.max_queued(), None, "abandoned rings are skipped");
    }

    #[test]
    fn blocking_push_stops_before_writing() {
        let (mut tx, mut rx) = broadcast_channel();
        let (prod, mut cons) = RingBuffer::<f32>::new(1024);
        let _ = tx.add(prod).expect("add");
        rx.apply_commands();

        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        // A normal push reaches the consumer.
        let block = vec![0.5f32; 512];
        rx.broadcast_blocking(&block, &stop, &paused, Duration::from_micros(100));
        let mut out = vec![0.0f32; 512];
        let n = pop_into(&mut cons, &mut out);
        assert_eq!(n, 512);
        assert_eq!(out, block);

        // A pre-existing stop refuses the next block.
        let stop2 = Arc::new(AtomicBool::new(true));
        rx.broadcast_blocking(
            &vec![0.5f32; 2048],
            &stop2,
            &paused,
            Duration::from_millis(1),
        );
        let mut stopped = vec![0.0f32; 512];
        assert_eq!(pop_into(&mut cons, &mut stopped), 0);
    }

    #[test]
    fn blocking_push_aborts_on_abandoned_consumer() {
        let (mut tx, mut rx) = broadcast_channel();
        let (prod, cons) = RingBuffer::<f32>::new(64);
        let _ = tx.add(prod).expect("add");
        rx.apply_commands();
        drop(cons);
        rx.apply_commands();
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        // The ring is abandoned: push must not hang.
        rx.broadcast_blocking(
            &vec![0.5f32; 2048],
            &stop,
            &paused,
            Duration::from_micros(50),
        );
    }
}
