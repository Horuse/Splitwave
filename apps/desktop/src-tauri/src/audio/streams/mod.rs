//! Cross-platform SPSC ring helpers plus the cpal stream builders.
//!
//! `bulk_push_counted` moves whole blocks into SPSC rings on every platform. The cpal `build_*_stream` builders back
//! macOS (CoreAudio) and Windows (WASAPI); Linux opens its mic via
//! `capture/linux.rs` and its speaker via `playback.rs`, so it skips them.

use std::sync::atomic::AtomicU64;

use rtrb::Producer;

use crate::audio::health;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod cpal_stream;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use cpal_stream::{build_input_stream, build_output_stream};

/// Bulk push via one `write_chunk` reservation -- one atomic-CAS per block
/// instead of one per sample. A block that does not fit is dropped whole
/// (the consumer is behind anyway; staying RT-safe beats blocking): a partial
/// write would split a frame, and every later frame would land a channel off.
/// Returns the number of samples written, so callers tracking fill level stay
/// in step with the ring.
///
/// `counter` names which ring this call site is feeding, so overruns can be
/// told apart by call site instead of summing into one global total.
pub fn bulk_push_counted(prod: &mut Producer<f32>, samples: &[f32], counter: &AtomicU64) -> usize {
    let want = samples.len();
    if want == 0 {
        return 0;
    }
    if prod.slots() < want {
        health::bump(counter, want as u64);
        return 0;
    }
    if let Ok(mut chunk) = prod.write_chunk(want) {
        let (first, second) = chunk.as_mut_slices();
        let n1 = first.len();
        first.copy_from_slice(&samples[..n1]);
        second.copy_from_slice(&samples[n1..]);
        chunk.commit_all();
    }
    want
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;
    use std::sync::atomic::Ordering;

    fn drain(cons: &mut rtrb::Consumer<f32>) -> Vec<f32> {
        let n = cons.slots();
        let chunk = cons.read_chunk(n).unwrap();
        let (a, b) = chunk.as_slices();
        let out: Vec<f32> = a.iter().chain(b).copied().collect();
        chunk.commit_all();
        out
    }

    // A mono source hands over odd sample counts all the time (a resampler
    // emitting 235 then 236 frames, 441-sample packets at 44.1 kHz). With room
    // in the ring every sample must land.
    #[test]
    fn odd_blocks_with_room_are_written_whole() {
        let (mut prod, mut cons) = RingBuffer::<f32>::new(10_000);
        let counter = AtomicU64::new(0);
        let mut fed = Vec::new();
        for (k, n) in [235usize, 236, 441, 1, 3, 70].iter().enumerate() {
            let block: Vec<f32> = (0..*n).map(|i| (k * 1000 + i) as f32).collect();
            assert_eq!(bulk_push_counted(&mut prod, &block, &counter), *n);
            fed.extend(block);
        }
        assert_eq!(counter.load(Ordering::Relaxed), 0, "nothing dropped");
        assert_eq!(drain(&mut cons), fed);
    }

    // On overflow a partial write would split a frame of any width and shift
    // every later frame's channels; the block is dropped whole instead.
    #[test]
    fn overflow_drops_the_whole_block_keeping_frames_aligned() {
        for width in [1usize, 2, 3, 6] {
            let (mut prod, mut cons) = RingBuffer::<f32>::new(10 * width);
            let counter = AtomicU64::new(0);
            let first: Vec<f32> = (0..8 * width).map(|i| i as f32).collect();
            assert_eq!(bulk_push_counted(&mut prod, &first, &counter), first.len());
            let second = vec![-1.0; 4 * width];
            assert_eq!(
                bulk_push_counted(&mut prod, &second, &counter),
                0,
                "{width}ch"
            );
            assert_eq!(counter.load(Ordering::Relaxed), (4 * width) as u64);
            let got = drain(&mut cons);
            assert_eq!(got, first, "{width}ch: ring holds whole frames only");
        }
    }
}
