//! Runs an expensive effect on its own thread so a processing spike cannot
//! cost the audio callback its deadline. The RT side only bulk-pushes into
//! and bulk-pops out of a pair of SPSC rings; the return ring's prefill (the
//! pad) is declared as latency so PDC aligns parallel branches against it.
//!
//! An effect that can only step in whole blocks of its own (a model hop) says
//! so, and the worker gathers exactly those before calling it; the effect
//! itself never buffers or pads for it. The pad is then what the gathering
//! and the worker's turnaround cost, at the worst alignment of the effect's
//! blocks against the engine's (see `pad_frames`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};
use tracing::warn;

use crate::audio::graph::MAX_BUFFER_FRAMES;
use crate::audio::health;

/// Floor for a worker's turnaround: waking it, scheduling slack, and the
/// processing itself.
const MIN_TURNAROUND_MS: f64 = 5.0;
/// Longest the worker runs without blocking. Linux charges real-time CPU time
/// between blocking calls against `RLIMIT_RTTIME` and kills the process past
/// it; a worker whose effect cannot keep up never runs out of input to park
/// on, so it sleeps briefly instead. It is starving the output either way.
const MAX_BUSY: Duration = Duration::from_millis(20);
const BREATHER: Duration = Duration::from_millis(1);

/// Smallest pad that never starves the RT side, for an effect stepping in
/// `working_frames` blocks driven by the engine in `block_frames` blocks.
///
/// The engine pushes a block and at once reads one back, `pad` frames older.
/// The effect's block `j` covers frames `[j*W, (j+1)*W)`; it is complete once
/// the engine block holding its last frame is pushed, and back a turnaround
/// later. Its first frame is read `pad` frames after it was pushed, on a block
/// boundary, so the pad must cover, for every `j`, the wait from pushing that
/// frame to the first engine block starting after the result is back. The
/// alignment of effect blocks against engine blocks repeats every
/// `lcm(W, B)` frames, so that one period is the whole story.
pub fn pad_frames(sample_rate: u32, block_frames: usize, working_frames: usize) -> usize {
    let b = block_frames.max(1);
    let w = working_frames.max(1);
    let turnaround = (sample_rate as f64 * MIN_TURNAROUND_MS / 1000.0).ceil() as usize;
    let turnaround_blocks = turnaround.div_ceil(b);
    let period = w / gcd(w, b) * b;
    (0..period / w)
        .map(|j| {
            let done = ((j + 1) * w - 1) / b;
            (done + turnaround_blocks) * b - j * w
        })
        .max()
        .unwrap_or(b)
        .max(b)
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Interleaved block processing, run on the offload thread.
pub trait BlockProcessor: Send {
    /// Consume `input` and append exactly `input.len()` samples to `output`.
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>);
}

pub struct Offload {
    to_worker: Producer<f32>,
    from_worker: Consumer<f32>,
    stop: Arc<AtomicBool>,
    /// Times the worker woke to find nothing to do.
    #[cfg(test)]
    idle_wakes: Arc<std::sync::atomic::AtomicUsize>,
    /// Samples the worker has taken in and, for whole effect blocks, answered.
    #[cfg(test)]
    taken: Arc<std::sync::atomic::AtomicUsize>,
    /// Times an overloaded worker stopped to block.
    #[cfg(test)]
    breathers: Arc<std::sync::atomic::AtomicUsize>,
    join: Option<JoinHandle<()>>,
    /// Woken after every push, so it sleeps until there is work instead of
    /// polling for it.
    worker: Thread,
    width: usize,
    pad_frames: usize,
}

// A partial write must not split a frame: a short tail would shift every later
// frame's channel order by one sample.
fn push_aligned(prod: &mut Producer<f32>, samples: &[f32], width: usize) -> usize {
    let want = samples.len();
    if want == 0 {
        return 0;
    }
    let avail = prod.slots();
    let to_write = want.min(avail) - want.min(avail) % width;
    health::bump(
        &health::OFFLOAD_RING_OVERRUN_SAMPLES,
        (want - to_write) as u64,
    );
    if to_write == 0 {
        return 0;
    }
    if let Ok(mut chunk) = prod.write_chunk(to_write) {
        let (first, second) = chunk.as_mut_slices();
        let n1 = first.len();
        first.copy_from_slice(&samples[..n1]);
        let n2 = second.len();
        if n2 > 0 {
            second.copy_from_slice(&samples[n1..n1 + n2]);
        }
        chunk.commit_all();
    }
    to_write
}

impl Offload {
    /// `working_frames` is the only block size `processor` can step in, if it
    /// has one; otherwise it is handed each engine block as it comes.
    pub fn spawn<P: BlockProcessor + 'static>(
        name: &'static str,
        processor: P,
        width: usize,
        working_frames: Option<usize>,
        block_frames: usize,
        sample_rate: u32,
    ) -> Result<Self, P> {
        if width == 0 {
            tracing::error!(name, "offload: width must be at least 1");
            return Err(processor);
        }
        let working = working_frames.unwrap_or(block_frames).max(1);
        let pad_frames = pad_frames(sample_rate, block_frames, working);

        // Room for the pad, a block in flight each way, and a stall's worth.
        let ring_frames = (pad_frames + working + MAX_BUFFER_FRAMES) * 4;
        let (to_worker, mut worker_in) = RingBuffer::<f32>::new(ring_frames * width);
        let (mut worker_out, from_worker) = RingBuffer::<f32>::new(ring_frames * width);

        match worker_out.write_chunk(pad_frames * width) {
            Ok(mut chunk) => {
                let (first, second) = chunk.as_mut_slices();
                first.fill(0.0);
                second.fill(0.0);
                chunk.commit_all();
            }
            Err(e) => {
                tracing::error!(name, error = %e, "offload: failed to prefill return ring pad");
                return Err(processor);
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        #[cfg(test)]
        let idle_wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        #[cfg(test)]
        let idle_wakes_thread = idle_wakes.clone();
        #[cfg(test)]
        let taken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        #[cfg(test)]
        let taken_thread = taken.clone();
        #[cfg(test)]
        let breathers = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        #[cfg(test)]
        let breathers_thread = breathers.clone();
        // Handoff cell rather than moving `processor` straight into the closure:
        // a failed `Builder::spawn` drops its closure internally with no way to
        // recover a moved value, so the cell is how the caller gets it back.
        let handoff = Arc::new(std::sync::Mutex::new(Some(processor)));
        let handoff_thread = handoff.clone();
        let join = match thread::Builder::new()
            .name(format!("offload:{name}"))
            .spawn(move || {
                let mut processor = handoff_thread
                    .lock()
                    .unwrap()
                    .take()
                    .expect("processor handed off");
                // The pad leaves this thread a few milliseconds per block, so
                // it must not wait behind ordinary threads.
                let _rt = crate::audio::pipeline::RtThread::promote(
                    "offload",
                    pad_frames as u32,
                    sample_rate,
                );
                // One of the effect's blocks, gathered across engine blocks.
                let mut gathered = vec![0.0f32; working * width];
                let mut filled = 0;
                let mut out = Vec::with_capacity(working * width);
                let mut busy_since = Instant::now();
                while !stop_thread.load(Ordering::Relaxed) {
                    let avail = worker_in.slots();
                    let avail = avail - avail % width; // whole frames only
                    if avail == 0 {
                        // A wake that lands before this parks leaves the
                        // token set, so the park returns at once.
                        thread::park();
                        busy_since = Instant::now();
                        #[cfg(test)]
                        idle_wakes_thread.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    let n = avail.min(gathered.len() - filled);
                    if let Ok(chunk) = worker_in.read_chunk(n) {
                        let (first, second) = chunk.as_slices();
                        let n1 = first.len();
                        gathered[filled..filled + n1].copy_from_slice(first);
                        let n2 = second.len();
                        if n2 > 0 {
                            gathered[filled + n1..filled + n1 + n2].copy_from_slice(second);
                        }
                        chunk.commit_all();
                    }
                    filled += n;
                    if filled == gathered.len() {
                        out.clear();
                        processor.process(&gathered, &mut out);
                        push_aligned(&mut worker_out, &out, width);
                        filled = 0;
                        if busy_since.elapsed() >= MAX_BUSY {
                            thread::sleep(BREATHER);
                            busy_since = Instant::now();
                            #[cfg(test)]
                            breathers_thread.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    #[cfg(test)]
                    taken_thread.fetch_add(n, Ordering::Relaxed);
                }
            }) {
            Ok(j) => j,
            Err(e) => {
                warn!(name, error = %e, "offload: failed to spawn worker thread");
                // The closure never ran, so it never took the cell's contents.
                let processor = handoff
                    .lock()
                    .unwrap()
                    .take()
                    .expect("processor handed off");
                return Err(processor);
            }
        };

        Ok(Self {
            to_worker,
            from_worker,
            stop,
            #[cfg(test)]
            idle_wakes,
            #[cfg(test)]
            taken,
            #[cfg(test)]
            breathers,
            worker: join.thread().clone(),
            join: Some(join),
            width,
            pad_frames,
        })
    }

    /// RT thread only: no allocations or locks. The one syscall is waking the
    /// worker when it sleeps, which never blocks.
    pub fn process(&mut self, samples: &mut [f32]) {
        let want = samples.len();
        if want == 0 {
            return;
        }
        push_aligned(&mut self.to_worker, samples, self.width);
        self.worker.unpark();

        // A starve leaves the return ring permanently deeper than the pad; trim
        // back so the declared latency stays true.
        let pad = self.pad_frames * self.width;
        let avail = self.from_worker.slots();
        if avail > want + pad {
            let excess = avail - want - pad;
            let excess = excess - excess % self.width;
            if excess > 0 {
                if let Ok(chunk) = self.from_worker.read_chunk(excess) {
                    chunk.commit_all();
                    health::bump(&health::OFFLOAD_RESYNC_DROPPED_SAMPLES, excess as u64);
                }
            }
        }

        let avail = self.from_worker.slots();
        let to_read = want.min(avail);
        if to_read > 0 {
            if let Ok(chunk) = self.from_worker.read_chunk(to_read) {
                let (first, second) = chunk.as_slices();
                let n1 = first.len();
                samples[..n1].copy_from_slice(first);
                let n2 = second.len();
                if n2 > 0 {
                    samples[n1..n1 + n2].copy_from_slice(second);
                }
                chunk.commit_all();
            }
        }
        for s in &mut samples[to_read..] {
            *s = 0.0;
        }
        health::bump(&health::OFFLOAD_STARVED_SAMPLES, (want - to_read) as u64);
    }

    pub fn latency_frames(&self) -> usize {
        self.pad_frames
    }
}

impl Drop for Offload {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.unpark();
        if let Some(j) = self.join.take() {
            if j.join().is_err() {
                warn!("offload: worker thread panicked");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hands its input straight back, checking it only ever gets whole blocks
    /// of `frames` when it has a block size of its own.
    struct Passthrough {
        frames: Option<usize>,
        width: usize,
    }

    fn passthrough() -> Passthrough {
        Passthrough {
            frames: None,
            width: 2,
        }
    }

    impl BlockProcessor for Passthrough {
        fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
            if let Some(frames) = self.frames {
                assert_eq!(input.len(), frames * self.width, "a partial block");
            }
            output.extend_from_slice(input);
        }
    }

    // Waits for the worker to hand back the block just pushed, so a slow
    // runner cannot starve the next `process` into zeros.
    fn wait_for_return(offload: &Offload, want: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while offload.from_worker.slots() < want {
            assert!(Instant::now() < deadline, "offload worker stalled");
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Waits until the worker has taken everything pushed so far and answered
    /// every whole block of it.
    fn wait_until_taken(offload: &Offload, pushed: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while offload.taken.load(Ordering::Relaxed) < pushed {
            assert!(Instant::now() < deadline, "offload worker stalled");
            thread::yield_now();
        }
    }

    fn roundtrip(width: usize, block: usize, working: Option<usize>, sample_rate: u32) {
        let processor = Passthrough {
            frames: working,
            width,
        };
        let Ok(mut offload) = Offload::spawn("test", processor, width, working, block, sample_rate)
        else {
            panic!("spawn offload")
        };
        let pad = offload.latency_frames();
        assert_eq!(
            pad,
            pad_frames(sample_rate, block, working.unwrap_or(block))
        );

        let mut fed = Vec::new();
        let mut got = Vec::new();
        let mut counter = 1.0f32;
        let blocks = (4 * (pad + working.unwrap_or(0)) / block).max(8);
        for _ in 0..blocks {
            let mut data = vec![0.0f32; block * width];
            for frame in data.chunks_exact_mut(width) {
                frame.fill(counter);
                counter += 1.0;
            }
            fed.extend_from_slice(&data);
            offload.process(&mut data);
            got.extend_from_slice(&data);
            wait_until_taken(&offload, fed.len());
        }

        let shift = pad * width;
        let case = format!("{block}/{working:?}");
        assert!(
            got[..shift].iter().all(|&v| v == 0.0),
            "{case}: pad is silence"
        );
        for i in shift..got.len() {
            assert_eq!(got[i], fed[i - shift], "{case}: mismatch at {i}");
        }
    }

    #[test]
    fn offload_delays_by_exactly_its_pad_at_every_block_size() {
        for block in crate::audio::graph::BUFFER_FRAME_OPTIONS.map(|n| n as usize) {
            roundtrip(2, block, None, 48_000);
        }
    }

    #[test]
    fn a_block_stepping_effect_gets_whole_blocks_and_an_exact_delay() {
        for block in [32, 64, 256, 1024] {
            roundtrip(2, block, Some(441), 44_100);
            roundtrip(2, block, Some(480), 48_000);
        }
    }

    #[test]
    fn offload_roundtrip_handles_wide_blocks() {
        roundtrip(6, 1024, None, 48_000);
        roundtrip(6, 64, Some(480), 48_000);
    }

    /// Whether reading with `pad` ever finds a frame not yet back, frame by
    /// frame, with the worker taking exactly `turnaround` frames of time.
    fn starves(pad: usize, block: usize, working: usize, turnaround: usize) -> bool {
        // Past the pad, then two full alignment periods.
        let blocks = (pad + working) / block + working / gcd(working, block) * 2 + 8;
        (0..blocks).any(|k| {
            (0..block).any(|i| {
                let Some(frame) = (k * block + i).checked_sub(pad) else {
                    return false;
                };
                let j = frame / working;
                let done = ((j + 1) * working - 1) / block;
                done * block + turnaround > k * block
            })
        })
    }

    #[test]
    fn the_pad_is_the_least_that_never_starves() {
        for sample_rate in [44_100, 48_000, 96_000] {
            let turnaround = (sample_rate as f64 * MIN_TURNAROUND_MS / 1000.0).ceil() as usize;
            for block in crate::audio::graph::BUFFER_FRAME_OPTIONS.map(|n| n as usize) {
                for working in [block, 441, 480, 960, 100, 1_500] {
                    let pad = pad_frames(sample_rate, block, working);
                    let case = format!("{sample_rate}/{block}/{working}");
                    assert!(
                        !starves(pad, block, working, turnaround),
                        "{case}: {pad} starves"
                    );
                    if pad > block {
                        assert!(
                            starves(pad - 1, block, working, turnaround),
                            "{case}: {pad} is more than needed"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_model_hop_costs_its_gathering_and_a_turnaround() {
        // DeepFilterNet at 44.1 kHz: 441-frame hops against 64-frame blocks.
        // The hop gathered, at worst misaligned by a block, plus 5 ms.
        let pad = pad_frames(44_100, 64, 441);
        assert!(pad <= 441 + 64 + 256, "{pad}");
        // An effect that takes any block pays the turnaround alone.
        assert_eq!(pad_frames(48_000, 32, 32), 256, "5 ms turnaround at 48k");
        assert_eq!(
            pad_frames(48_000, 2048, 2048),
            2048,
            "never below the block"
        );
    }

    /// Takes `cost` of CPU per block, never blocking: an effect slower than
    /// real time.
    struct Slow {
        cost: Duration,
    }

    impl BlockProcessor for Slow {
        fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
            let started = Instant::now();
            while started.elapsed() < self.cost {
                std::hint::spin_loop();
            }
            output.extend_from_slice(input);
        }
    }

    #[test]
    fn an_overloaded_worker_still_blocks_now_and_then() {
        // A backlog it cannot clear keeps it from ever finding its input
        // empty; it must still block within the busy limit.
        let slow = Slow {
            cost: Duration::from_millis(2),
        };
        let Ok(mut offload) = Offload::spawn("test", slow, 2, None, 64, 48_000) else {
            panic!("spawn offload")
        };
        let mut data = vec![0.0f32; 64 * 2];
        for _ in 0..60 {
            offload.process(&mut data);
        }
        thread::sleep(Duration::from_millis(100));
        let breathers = offload.breathers.load(Ordering::Relaxed);
        assert!(
            breathers >= 2,
            "ran {breathers} breathers in 100 ms of backlog"
        );
    }

    #[test]
    fn an_idle_worker_sleeps_until_woken() {
        // A 1 ms poll wakes ~100 times in 100 ms; a parked worker only when a
        // push wakes it (or, rarely, spuriously).
        let Ok(mut offload) = Offload::spawn("test", passthrough(), 2, None, 64, 48_000) else {
            panic!("spawn offload")
        };
        thread::sleep(Duration::from_millis(100));
        let idle = offload.idle_wakes.load(Ordering::Relaxed);
        assert!(idle <= 2, "woke {idle} times with nothing to do");
        // And a push is answered without any poll to pick it up.
        let mut data = vec![0.5f32; 64 * 2];
        offload.process(&mut data);
        wait_for_return(&offload, data.len());
    }

    #[test]
    fn processing_never_touches_the_heap() {
        let Ok(mut offload) = Offload::spawn("test", passthrough(), 2, Some(441), 64, 44_100)
        else {
            panic!("spawn offload")
        };
        let mut data = vec![0.25f32; 64 * 2];
        crate::audio::rt_guard::assert_no_alloc("offload process", || {
            for _ in 0..2_000 {
                offload.process(&mut data);
            }
        });
    }

    #[test]
    fn dropping_wakes_a_sleeping_worker() {
        let Ok(offload) = Offload::spawn("test", passthrough(), 2, None, 64, 48_000) else {
            panic!("spawn offload")
        };
        thread::sleep(Duration::from_millis(20));
        let started = Instant::now();
        drop(offload);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "join waited on a parked worker"
        );
    }
}
