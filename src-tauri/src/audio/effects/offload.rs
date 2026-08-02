//! Runs an expensive effect on its own thread so a processing spike cannot
//! cost the DSP worker its block deadline. The RT side only bulk-pushes into
//! and bulk-pops out of a pair of SPSC rings; the return ring's prefill is
//! declared as latency so PDC aligns parallel branches against it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};
use tracing::warn;

use crate::audio::health;
use crate::audio::pipeline::dag::DSP_BLOCK_FRAMES;

// The RT side pops a block immediately after pushing it, so the return ring
// must already hold a full block: that prefill is the offload's latency.
const PAD_FRAMES: usize = DSP_BLOCK_FRAMES;
const RING_FRAMES: usize = DSP_BLOCK_FRAMES * 16;
const POLL_INTERVAL: Duration = Duration::from_millis(1);

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
    health::bump(&health::OFFLOAD_RING_OVERRUN_SAMPLES, (want - to_write) as u64);
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
    pub fn spawn<P: BlockProcessor + 'static>(
        name: &'static str,
        processor: P,
        width: usize,
    ) -> Result<Self, P> {
        if width == 0 {
            tracing::error!(name, "offload: width must be at least 1");
            return Err(processor);
        }

        let (to_worker, mut worker_in) = RingBuffer::<f32>::new(RING_FRAMES * width);
        let (mut worker_out, from_worker) = RingBuffer::<f32>::new(RING_FRAMES * width);

        match worker_out.write_chunk(PAD_FRAMES * width) {
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
        let join = match thread::Builder::new().name(format!("offload:{name}")).spawn(move || {
            let mut processor = handoff_thread.lock().unwrap().take().expect("processor handed off");
            let mut scratch = vec![0.0f32; DSP_BLOCK_FRAMES * width];
            let mut out = Vec::with_capacity(DSP_BLOCK_FRAMES * width);
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
                let processor = handoff.lock().unwrap().take().expect("processor handed off");
                return Err(processor);
            }
        };

        Ok(Self { to_worker, from_worker, stop, join: Some(join), width })
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
        let pad = PAD_FRAMES * self.width;
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
        PAD_FRAMES
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

    #[test]
    fn offload_roundtrip_delays_by_pad() {
        let Ok(mut offload) = Offload::spawn("test", Passthrough, 2) else { panic!("spawn offload") };

        let mut fed = Vec::new();
        let mut got = Vec::new();
        let mut counter = 0.0f32;

        for _ in 0..8 {
            let mut block = vec![0.0f32; DSP_BLOCK_FRAMES * 2];
            for f in 0..DSP_BLOCK_FRAMES {
                block[f * 2] = counter;
                block[f * 2 + 1] = counter;
                counter += 1.0;
            }
            fed.extend_from_slice(&block);
            offload.process(&mut block);
            got.extend_from_slice(&block);
            thread::sleep(Duration::from_millis(30));
        }

        let pad = PAD_FRAMES * 2;
        for (i, &v) in got.iter().enumerate().take(pad) {
            assert_eq!(v, 0.0, "expected zero pad at index {i}");
        }
        for i in pad..got.len() {
            assert_eq!(got[i], fed[i - pad], "mismatch at index {i}");
        }
    }

    #[test]
    fn offload_roundtrip_handles_wide_blocks() {
        const WIDTH: usize = 6;
        let Ok(mut offload) = Offload::spawn("test-wide", Passthrough, WIDTH) else {
            panic!("spawn offload")
        };

        let mut fed = Vec::new();
        let mut got = Vec::new();
        let mut counter = 0.0f32;

        for _ in 0..8 {
            let mut block = vec![0.0f32; DSP_BLOCK_FRAMES * WIDTH];
            for f in 0..DSP_BLOCK_FRAMES {
                for c in 0..WIDTH {
                    block[f * WIDTH + c] = counter;
                }
                counter += 1.0;
            }
            fed.extend_from_slice(&block);
            offload.process(&mut block);
            got.extend_from_slice(&block);
            thread::sleep(Duration::from_millis(30));
        }

        let pad = PAD_FRAMES * WIDTH;
        for (i, &v) in got.iter().enumerate().take(pad) {
            assert_eq!(v, 0.0, "expected zero pad at index {i}");
        }
        for i in pad..got.len() {
            assert_eq!(got[i], fed[i - pad], "mismatch at index {i}");
        }
    }
}
