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
use crate::audio::streams::bulk_push_counted;

// The RT side pops a block immediately after pushing it, so the return ring
// must already hold a full block: that prefill is the offload's latency.
const PAD_FRAMES: usize = DSP_BLOCK_FRAMES;
const RING_FRAMES: usize = DSP_BLOCK_FRAMES * 16;
const POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Stereo-interleaved block processing, run on the offload thread.
pub trait BlockProcessor: Send {
    /// Consume `input` and append exactly `input.len()` samples to `output`.
    fn process(&mut self, input: &[f32], output: &mut Vec<f32>);
}

pub struct Offload {
    to_worker: Producer<f32>,
    from_worker: Consumer<f32>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Offload {
    pub fn spawn<P: BlockProcessor + 'static>(name: &'static str, mut processor: P) -> Option<Self> {
        let (to_worker, mut worker_in) = RingBuffer::<f32>::new(RING_FRAMES * 2);
        let (mut worker_out, from_worker) = RingBuffer::<f32>::new(RING_FRAMES * 2);

        match worker_out.write_chunk(PAD_FRAMES * 2) {
            Ok(mut chunk) => {
                let (first, second) = chunk.as_mut_slices();
                first.fill(0.0);
                second.fill(0.0);
                chunk.commit_all();
            }
            Err(e) => {
                tracing::error!(name, error = %e, "offload: failed to prefill return ring pad");
                return None;
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        let join = match thread::Builder::new().name(format!("offload:{name}")).spawn(move || {
            let mut scratch = vec![0.0f32; DSP_BLOCK_FRAMES * 2];
            let mut out = Vec::with_capacity(DSP_BLOCK_FRAMES * 2);
            while !stop_thread.load(Ordering::Relaxed) {
                let avail = worker_in.slots() & !1; // whole stereo frames only
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
                bulk_push_counted(&mut worker_out, &out, &health::OFFLOAD_RING_OVERRUN_SAMPLES);
            }
        }) {
            Ok(j) => j,
            Err(e) => {
                warn!(name, error = %e, "offload: failed to spawn worker thread");
                return None;
            }
        };

        Some(Self { to_worker, from_worker, stop, join: Some(join) })
    }

    /// RT thread only: no allocations, locks, or syscalls.
    pub fn process(&mut self, samples: &mut [f32]) {
        let want = samples.len();
        if want == 0 {
            return;
        }
        bulk_push_counted(&mut self.to_worker, samples, &health::OFFLOAD_RING_OVERRUN_SAMPLES);

        // A starve leaves the return ring permanently deeper than the pad; trim
        // back so the declared latency stays true.
        let pad = PAD_FRAMES * 2;
        let avail = self.from_worker.slots();
        if avail > want + pad {
            let excess = (avail - want - pad) & !1;
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
        let mut offload = Offload::spawn("test", Passthrough).expect("spawn offload");

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
}
