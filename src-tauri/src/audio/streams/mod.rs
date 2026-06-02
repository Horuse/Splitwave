//! Cross-platform SPSC ring helpers plus the cpal stream builders (macOS).
//!
//! `bulk_pop` / `bulk_push` move whole blocks between the pipeline and the
//! audio callback on every platform. The cpal `build_*_stream` builders are
//! macOS-only; Linux opens its mic via `capture/linux.rs` and its speaker via
//! `playback.rs`, so it needs no builders here.

use rtrb::Producer;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{build_input_stream, build_output_stream};

/// Bulk drain `dst.len()` samples from an SPSC ring. Anything we couldn't
/// read (consumer faster than producer) is zero-filled -- that's the device
/// playing silence, not glitching.
pub fn bulk_pop(cons: &mut rtrb::Consumer<f32>, dst: &mut [f32]) {
    let want = dst.len();
    if want == 0 {
        return;
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
}

/// Bulk push via one `write_chunk` reservation -- one atomic-CAS per block
/// instead of one per sample. On overflow only the head fits and the rest is
/// dropped (consumer is behind anyway; staying RT-safe beats blocking).
pub fn bulk_push(prod: &mut Producer<f32>, samples: &[f32]) {
    let want = samples.len();
    if want == 0 {
        return;
    }
    let avail = prod.slots();
    let to_write = want.min(avail);
    if to_write == 0 {
        return;
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
}
