//! Cross-platform SPSC ring helpers plus the cpal stream builders.
//!
//! `bulk_pop` / `bulk_push_counted` move whole blocks between the pipeline and the
//! audio callback on every platform. The cpal `build_*_stream` builders back
//! macOS (CoreAudio) and Windows (WASAPI); Linux opens its mic via
//! `capture/linux.rs` and its speaker via `playback.rs`, so it skips them.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use rtrb::Producer;

use crate::audio::health;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod cpal_stream;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use cpal_stream::{build_input_stream, build_output_stream, build_raw_input_stream};

#[derive(Clone)]
pub struct RawCaptureStats {
    pub captured_samples: Arc<AtomicU64>,
    pub dropped_samples: Arc<AtomicU64>,
}

impl RawCaptureStats {
    pub fn new() -> Self {
        Self {
            captured_samples: Arc::new(AtomicU64::new(0)),
            dropped_samples: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Bulk drain `dst.len()` samples from an SPSC ring. Anything we couldn't
/// read (consumer faster than producer) is zero-filled -- that's the device
/// playing silence, not glitching. Returns the number of samples actually
/// read, so callers tracking ring fill level can subtract it.
pub fn bulk_pop(cons: &mut rtrb::Consumer<f32>, dst: &mut [f32]) -> usize {
    let want = dst.len();
    if want == 0 {
        return 0;
    }
    let avail = cons.slots();
    let to_read = want.min(avail);
    if to_read > 0 {
        if let Ok(chunk) = cons.read_chunk(to_read) {
            let (first, second) = chunk.as_slices();
            let n1 = first.len();
            dst[..n1].copy_from_slice(first);
            let n2 = second.len();
            if n2 > 0 {
                dst[n1..n1 + n2].copy_from_slice(second);
            }
            chunk.commit_all();
        }
    }
    for s in &mut dst[to_read..] {
        *s = 0.0;
    }
    health::bump(&health::OUTPUT_UNDERRUN_SAMPLES, (want - to_read) as u64);
    to_read
}

/// Bulk push via one `write_chunk` reservation -- one atomic-CAS per block
/// instead of one per sample. On overflow only the head fits and the rest is
/// dropped (consumer is behind anyway; staying RT-safe beats blocking).
/// Returns the number of samples actually written, so callers tracking ring
/// fill level stay in step with the ring after a dropped tail.
///
/// `counter` names which ring this call site is feeding, so overruns can be
/// told apart by call site instead of summing into one global total.
pub fn bulk_push_counted(prod: &mut Producer<f32>, samples: &[f32], counter: &AtomicU64) -> usize {
    bulk_push_frames_counted(prod, samples, 2, counter)
}

/// Bulk push complete interleaved frames. Array capture uses this instead of
/// the stereo helper so an overrun never separates channels from one physical
/// device stream.
pub fn bulk_push_frames_counted(
    prod: &mut Producer<f32>,
    samples: &[f32],
    channels: usize,
    counter: &AtomicU64,
) -> usize {
    let want = samples.len();
    if want == 0 || channels == 0 {
        return 0;
    }
    let avail = prod.slots();
    let to_write = want.min(avail) / channels * channels;
    health::bump(counter, (want - to_write) as u64);
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
