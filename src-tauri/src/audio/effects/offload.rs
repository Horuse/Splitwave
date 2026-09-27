//! Runs an expensive effect on its own thread so a processing spike cannot
//! cost the audio callback its deadline. The RT side only bulk-pushes into
//! and bulk-pops out of a pair of SPSC rings; the return ring's prefill (the
//! pad) is declared as latency so PDC aligns parallel branches against it.
//!
//! The pad is the effect's working block: it is how long the worker thread
//! may take to hand a block back. It is never below the engine block (the RT
//! side reads a whole block at once) and never below the effect's own floor.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};
use tracing::warn;

use crate::audio::graph::MAX_BUFFER_FRAMES;
use crate::audio::health;

const POLL_INTERVAL: Duration = Duration::from_millis(1);
/// Floor for a worker's turnaround: its 1 ms poll plus scheduling slack plus
/// the processing itself.
const MIN_TURNAROUND_MS: f64 = 5.0;

/// Pad for an offloaded effect at `sample_rate` driven in `block_frames`
/// blocks, given the effect's own processing floor. A power of two, so it
/// reads as a buffer size.
pub fn pad_frames(sample_rate: u32, block_frames: usize, floor_frames: usize) -> usize {
    let turnaround = (sample_rate as f64 * MIN_TURNAROUND_MS / 1000.0).ceil() as usize;
    block_frames
        .max(turnaround.next_power_of_two())
        .max(floor_frames)
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
    join: Option<JoinHandle<()>>,
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
    /// `pad_frames` comes from [`pad_frames`]; `sample_rate` sizes the
    /// worker's real-time scheduling.
    pub fn spawn<P: BlockProcessor + 'static>(
        name: &'static str,
        processor: P,
        width: usize,
        pad_frames: usize,
        sample_rate: u32,
    ) -> Result<Self, P> {
        if width == 0 {
            tracing::error!(name, "offload: width must be at least 1");
            return Err(processor);
        }

        // Room for the pad, a block in flight each way, and a stall's worth.
        let ring_frames = (pad_frames + MAX_BUFFER_FRAMES) * 4;
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
                let mut scratch = vec![0.0f32; MAX_BUFFER_FRAMES * width];
                let mut out = Vec::with_capacity(MAX_BUFFER_FRAMES * width);
                while !stop_thread.load(Ordering::Relaxed) {
                    let avail = worker_in.slots();
                    let avail = avail - avail % width; // whole frames only
                    if avail == 0 {
                        thread::sleep(POLL_INTERVAL);
                        continue;
                    }
                    let n = avail.min(scratch.len());
                    if let Ok(chunk) = worker_in.read_chunk(n) {
                        let (first, second) = chunk.as_slices();
                        let n1 = first.len();
                        scratch[..n1].copy_from_slice(first);
                        let n2 = second.len();
                        if n2 > 0 {
                            scratch[n1..n1 + n2].copy_from_slice(second);
                        }
                        chunk.commit_all();
                    }
                    out.clear();
                    processor.process(&scratch[..n], &mut out);
                    push_aligned(&mut worker_out, &out, width);
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
            join: Some(join),
            width,
            pad_frames,
        })
    }

    /// RT thread only: no allocations, locks, or syscalls.
    pub fn process(&mut self, samples: &mut [f32]) {
        let want = samples.len();
        if want == 0 {
            return;
        }
        push_aligned(&mut self.to_worker, samples, self.width);

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

    struct Passthrough;

    impl BlockProcessor for Passthrough {
        fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
            output.extend_from_slice(input);
        }
    }

    // Waits for the worker to hand back the block just pushed, so a slow
    // runner cannot starve the next `process` into zeros.
    fn wait_for_return(offload: &Offload, want: usize) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while offload.from_worker.slots() < want {
            assert!(
                std::time::Instant::now() < deadline,
                "offload worker stalled"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn roundtrip(width: usize, block: usize, pad: usize) {
        let Ok(mut offload) = Offload::spawn("test", Passthrough, width, pad, 48_000) else {
            panic!("spawn offload")
        };
        assert_eq!(offload.latency_frames(), pad);

        let mut fed = Vec::new();
        let mut got = Vec::new();
        let mut counter = 1.0f32;
        let blocks = (4 * pad / block).max(8);
        for _ in 0..blocks {
            let mut data = vec![0.0f32; block * width];
            for frame in data.chunks_exact_mut(width) {
                frame.fill(counter);
                counter += 1.0;
            }
            fed.extend_from_slice(&data);
            offload.process(&mut data);
            got.extend_from_slice(&data);
            wait_for_return(&offload, data.len());
        }

        let shift = pad * width;
        assert!(got[..shift].iter().all(|&v| v == 0.0), "pad is silence");
        for i in shift..got.len() {
            assert_eq!(got[i], fed[i - shift], "{block}/{pad}: mismatch at {i}");
        }
    }

    #[test]
    fn offload_delays_by_exactly_its_pad_at_every_block_size() {
        for block in crate::audio::graph::BUFFER_FRAME_OPTIONS.map(|n| n as usize) {
            roundtrip(2, block, pad_frames(48_000, block, 0));
        }
    }

    #[test]
    fn offload_roundtrip_handles_wide_blocks() {
        roundtrip(6, 1024, 1024);
    }

    #[test]
    fn pad_never_drops_below_the_block_or_the_floor() {
        assert_eq!(pad_frames(48_000, 32, 0), 256, "5 ms turnaround at 48k");
        assert_eq!(pad_frames(96_000, 32, 0), 512);
        assert_eq!(pad_frames(44_100, 64, 0), 256);
        assert_eq!(pad_frames(48_000, 2048, 0), 2048, "never below the block");
        assert_eq!(pad_frames(48_000, 64, 480), 480, "effect's own floor");
    }
}
