use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::json;
use tracing::{info, warn};

use crate::audio::effects::{GrHandle, LufsHandle, MeterHandle, WaveformHandle};
use crate::audio::health;

use super::dag::{OutputMeta, SourceMeta};
use super::host::Host;
use super::output::LIVE_SPEAKER_STREAMS;

const METER_EVENT: &str = "audio://meter";
const LUFS_EVENT: &str = "audio://lufs";
const GR_EVENT: &str = "audio://gr";
const SCOPE_EVENT: &str = "audio://scope";
const METER_TICK: Duration = Duration::from_millis(33);

const XRUN_TICK: Duration = Duration::from_millis(1000);
/// Fraction a measured rate may drift from its real-time expectation before
/// it's worth a log line; below this, scheduler jitter is normal.
const RATE_DEVIATION: f64 = 0.02;

/// True when `measured` has drifted from `expected` by more than both the
/// relative tolerance and the counter's own step size. Counters advance one
/// whole block at a time, so a window boundary always misattributes up to one
/// of them: at 1024 frames that alone is 2.1% of a second, which
/// `RATE_DEVIATION` would flag on every healthy run.
fn off_rate(measured: f64, expected: f64, quantum: f64) -> bool {
    expected > 0.0 && (measured - expected).abs() > (expected * RATE_DEVIATION).max(quantum * 1.5)
}

pub(super) struct XrunTickThread {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for XrunTickThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            // Woken rather than waited out: a reconcile drops this thread and
            // must not sit through the rest of its tick.
            j.thread().unpark();
            let _ = j.join();
        }
    }
}

/// Polls per-source and per-output counters once a second and logs whatever
/// looks wrong: a growing xrun/trim count, or a consumed/produced rate that
/// has drifted from real time by more than `RATE_DEVIATION`. A healthy run
/// stays silent. Real elapsed time is measured with `Instant` rather than
/// assumed to be exactly `XRUN_TICK`, since `thread::sleep` only guarantees
/// "at least".
pub(super) fn spawn_xrun_thread(
    sources: Vec<SourceMeta>,
    outputs: Vec<OutputMeta>,
    expected_speaker_streams: i64,
) -> XrunTickThread {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let join = thread::Builder::new()
        .name("xrun-tick".into())
        .spawn(move || {
            let mut last_xrun: Vec<u64> = sources
                .iter()
                .map(|s| s.stats.xrun.load(Ordering::Relaxed))
                .collect();
            let mut last_stalled: Vec<u64> = sources
                .iter()
                .map(|s| s.stats.stalled.load(Ordering::Relaxed))
                .collect();
            let mut last_trimmed: Vec<u64> = sources
                .iter()
                .map(|s| s.stats.trimmed.load(Ordering::Relaxed))
                .collect();
            let mut last_consumed: Vec<u64> = sources
                .iter()
                .map(|s| s.stats.consumed.load(Ordering::Relaxed))
                .collect();
            let mut last_fed: Vec<u64> = sources
                .iter()
                .map(|s| {
                    s.capture
                        .as_ref()
                        .map_or(0, |c| c.fed.load(Ordering::Relaxed))
                })
                .collect();
            let mut last_dropped: Vec<u64> = sources
                .iter()
                .map(|s| {
                    s.capture
                        .as_ref()
                        .map_or(0, |c| c.dropped.load(Ordering::Relaxed))
                })
                .collect();
            let mut announced: Vec<bool> = vec![false; sources.len()];
            let mut last_failed: Vec<u64> = vec![0; sources.len()];
            let mut last_blocks: Vec<u64> = outputs
                .iter()
                .map(|o| o.blocks.load(Ordering::Relaxed))
                .collect();
            let mut last_requested: Vec<u64> = outputs
                .iter()
                .map(|o| {
                    o.io.as_ref()
                        .map_or(0, |io| io.requested.load(Ordering::Relaxed))
                })
                .collect();
            let mut last_overloads: Vec<u64> = outputs
                .iter()
                .map(|o| {
                    o.io.as_ref()
                        .map_or(0, |io| io.overloads.load(Ordering::Relaxed))
                })
                .collect();
            let mut last_callbacks: Vec<u64> = outputs
                .iter()
                .map(|o| {
                    o.io.as_ref()
                        .map_or(0, |io| io.callbacks.load(Ordering::Relaxed))
                })
                .collect();
            let mut last_global: Vec<u64> = health::snapshot().iter().map(|(_, v)| *v).collect();
            let mut last_tick = Instant::now();
            // The first window spans the pipeline coming up: rings prefill and
            // sources come online tens of ms apart, so its deltas describe the
            // startup transient, not a defect. Baselines still advance through
            // it so the second window starts clean.
            let mut warmup = true;
            while !stop_thread.load(Ordering::SeqCst) {
                thread::park_timeout(XRUN_TICK);
                if stop_thread.load(Ordering::SeqCst) {
                    break;
                }
                let now = Instant::now();
                let elapsed_secs = now.duration_since(last_tick).as_secs_f64();
                last_tick = now;

                for (i, s) in sources.iter().enumerate() {
                    let xrun_now = s.stats.xrun.load(Ordering::Relaxed);
                    let stalled_now = s.stats.stalled.load(Ordering::Relaxed);
                    let trimmed_now = s.stats.trimmed.load(Ordering::Relaxed);
                    let consumed_now = s.stats.consumed.load(Ordering::Relaxed);
                    // Logged here rather than on the audio thread that sees it.
                    if !announced[i] && s.stats.online.load(Ordering::Relaxed) {
                        announced[i] = true;
                        info!(source = %s.label, "source online");
                    }
                    let failed_now = s.stats.failed.load(Ordering::Relaxed);
                    if failed_now > last_failed[i] {
                        warn!(
                            source = %s.label,
                            chunks = failed_now - last_failed[i],
                            "source resampler failed; chunks dropped"
                        );
                        last_failed[i] = failed_now;
                    }
                    let xrun_delta = xrun_now.saturating_sub(last_xrun[i]);
                    let stalled_delta = stalled_now.saturating_sub(last_stalled[i]);
                    let trimmed_delta = trimmed_now.saturating_sub(last_trimmed[i]);
                    let consumed_delta = consumed_now.saturating_sub(last_consumed[i]);
                    last_xrun[i] = xrun_now;
                    last_stalled[i] = stalled_now;
                    last_trimmed[i] = trimmed_now;
                    last_consumed[i] = consumed_now;

                    // Capture-side fed/dropped, only present for sources fed by a
                    // capture broadcast (mic/system-audio/app/file); ring-sources
                    // and network producers stay `None`.
                    let capture_delta = s.capture.as_ref().map(|c| {
                        let fed_now = c.fed.load(Ordering::Relaxed);
                        let dropped_now = c.dropped.load(Ordering::Relaxed);
                        let fed_delta = fed_now.saturating_sub(last_fed[i]);
                        let dropped_delta = dropped_now.saturating_sub(last_dropped[i]);
                        last_fed[i] = fed_now;
                        last_dropped[i] = dropped_now;
                        (fed_delta, dropped_delta)
                    });
                    let dropped_delta = capture_delta.map_or(0, |(_, d)| d);

                    // Audio removed by design (the startup backlog and depth
                    // correction, late audio realigned under silence already
                    // played) was consumed but never played.
                    let consumed_frames =
                        consumed_delta.saturating_sub(trimmed_delta) / s.channels.max(1) as u64;
                    let wallclock_frames = s.native_sr as f64 * elapsed_secs;
                    // A capture-backed source is measured against what its
                    // producer actually delivered: an app playing nothing feeds
                    // nothing, and the question here is whether the pipeline
                    // keeps up with its inputs, not whether an input is busy.
                    let expected_frames = match capture_delta {
                        Some((fed_delta, _)) => (fed_delta / s.channels.max(1) as u64) as f64,
                        None => wallclock_frames,
                    };
                    let off_rate = off_rate(
                        consumed_frames as f64,
                        expected_frames,
                        s.frames_per_block as f64,
                    );
                    // Under-delivery makes every stall and xrun in this window
                    // the designed response to an idle source. It also hides a
                    // capture that is genuinely failing, which surfaces through
                    // its own stream-error and source-online logging instead.
                    let producer_short = capture_delta.is_some_and(|_| {
                        expected_frames < wallclock_frames - s.frames_per_block as f64
                    });

                    if !warmup
                        && !producer_short
                        && (xrun_delta > 0 || stalled_delta > 0 || off_rate || dropped_delta > 0)
                    {
                        let ring_level_samples = s.stats.level.load(Ordering::Relaxed);
                        match capture_delta {
                            Some((fed_delta, dropped_delta)) => {
                                let fed_frames = fed_delta / s.channels.max(1) as u64;
                                warn!(
                                    source = %s.label,
                                    consumed_frames,
                                    expected_frames = wallclock_frames.round() as u64,
                                    trimmed_samples = trimmed_delta,
                                    xrun_samples = xrun_delta,
                                    stalled_samples = stalled_delta,
                                    ring_level_samples,
                                    fed_frames,
                                    dropped_samples = dropped_delta,
                                    "source rate anomaly"
                                );
                            }
                            None => {
                                warn!(
                                    source = %s.label,
                                    consumed_frames,
                                    expected_frames = wallclock_frames.round() as u64,
                                    trimmed_samples = trimmed_delta,
                                    xrun_samples = xrun_delta,
                                    stalled_samples = stalled_delta,
                                    ring_level_samples,
                                    "source rate anomaly"
                                );
                            }
                        }
                    }
                }

                for (i, o) in outputs.iter().enumerate() {
                    let blocks_now = o.blocks.load(Ordering::Relaxed);
                    let blocks_delta = blocks_now.saturating_sub(last_blocks[i]);
                    last_blocks[i] = blocks_now;

                    let expected_blocks =
                        o.sample_rate as f64 / o.block_frames as f64 * elapsed_secs;
                    let blocks_off_rate = off_rate(blocks_delta as f64, expected_blocks, 1.0);

                    // Device-pull diagnostics: tells "device asking for far more
                    // than real time implies" apart from "graph too slow for the
                    // buffer", which shows as overloads.
                    let io = o.io.as_ref().map(|io| {
                        let requested_now = io.requested.load(Ordering::Relaxed);
                        let overloads_now = io.overloads.load(Ordering::Relaxed);
                        let callbacks_now = io.callbacks.load(Ordering::Relaxed);
                        let requested_delta = requested_now.saturating_sub(last_requested[i]);
                        let overloads_delta = overloads_now.saturating_sub(last_overloads[i]);
                        let callbacks_delta = callbacks_now.saturating_sub(last_callbacks[i]);
                        last_requested[i] = requested_now;
                        last_overloads[i] = overloads_now;
                        last_callbacks[i] = callbacks_now;
                        (requested_delta, overloads_delta, callbacks_delta)
                    });
                    let io_off_rate = io.is_some_and(|(requested_delta, _, callbacks_delta)| {
                        let expected_samples =
                            o.io.as_ref().map_or(o.sample_rate, |s| s.sample_rate) as f64
                                * o.channels as f64
                                * elapsed_secs;
                        // The device's own buffer size, measured rather than
                        // assumed: the device may not grant the requested one.
                        let quantum = if callbacks_delta > 0 {
                            requested_delta as f64 / callbacks_delta as f64
                        } else {
                            0.0
                        };
                        off_rate(requested_delta as f64, expected_samples, quantum)
                    });
                    let overloaded = io.is_some_and(|(_, overloads, _)| overloads > 0);

                    if !warmup && (blocks_off_rate || io_off_rate || overloaded) {
                        match io {
                            Some((requested_samples, overloads, callbacks)) => warn!(
                                output = %o.label,
                                blocks = blocks_delta,
                                expected_blocks = expected_blocks.round() as u64,
                                requested_samples,
                                overloads,
                                callbacks,
                                "output block rate anomaly"
                            ),
                            None => warn!(
                                output = %o.label,
                                blocks = blocks_delta,
                                expected_blocks = expected_blocks.round() as u64,
                                "output block rate anomaly"
                            ),
                        }
                    }
                }

                // A stream that outlived its handle keeps calling back into a
                // renderer nobody owns, which shows in no output's own counters.
                let live_streams = LIVE_SPEAKER_STREAMS.load(Ordering::Relaxed);
                if live_streams != expected_speaker_streams {
                    warn!(
                        live_speaker_streams = live_streams,
                        expected_speaker_streams, "orphan speaker streams"
                    );
                }

                // High-water mark, not a running total: read-and-reset so the
                // next window reports its own worst miss rather than this one's.
                let worst_late_us = health::CLOCK_LATE_MAX_US.swap(0, Ordering::Relaxed);
                for (i, (name, now)) in health::snapshot().iter().enumerate() {
                    if *name == health::CLOCK_LATE_MAX_US_NAME {
                        continue;
                    }
                    let delta = now.saturating_sub(last_global[i]);
                    last_global[i] = *now;
                    if !warmup && delta > 0 {
                        warn!(counter = %name, delta, total = now, worst_late_us, "audio glitch");
                    }
                }
                warmup = false;
            }
        })
        .expect("spawn xrun tick thread");
    XrunTickThread {
        stop,
        join: Some(join),
    }
}

pub(super) struct MeterTickThread {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for MeterTickThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub(super) fn spawn_meter_thread(
    host: Host,
    meters: Vec<MeterHandle>,
    lufs: Vec<LufsHandle>,
    gr_handles: Vec<GrHandle>,
    scopes: Vec<WaveformHandle>,
) -> MeterTickThread {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let join = thread::Builder::new()
        .name("meter-tick".into())
        .spawn(move || {
            while !stop_thread.load(Ordering::SeqCst) {
                thread::sleep(METER_TICK);
                for m in &meters {
                    let snap = m.snapshot_and_decay();
                    host.emit(
                        METER_EVENT,
                        json!({
                            "nodeId": m.node_id,
                            "peaks": snap.peaks,
                            "rms": snap.rms,
                        }),
                    );
                }
                for l in &lufs {
                    let snap = l.snapshot();
                    host.emit(
                        LUFS_EVENT,
                        json!({
                            "nodeId": l.node_id,
                            "momentary": snap.momentary,
                            "shortterm": snap.shortterm,
                            "integrated": snap.integrated,
                            "tpL": snap.tp_l,
                            "tpR": snap.tp_r,
                            "lra": snap.lra,
                            "rms": snap.rms,
                            "noiseFloor": snap.noise_floor,
                            "samplePeak": snap.sample_peak,
                            "dcOffset": snap.dc_offset,
                            "correlation": snap.correlation,
                            "clips": snap.clips,
                        }),
                    );
                }
                for g in &gr_handles {
                    let gr_lin =
                        f32::from_bits(g.gr_lin.load(std::sync::atomic::Ordering::Relaxed));
                    host.emit(GR_EVENT, json!({ "nodeId": g.node_id, "grLin": gr_lin }));
                }
                for s in &scopes {
                    // Scopes emit a delta since the last tick; spectrum emits the
                    // full contiguous window it needs for its FFT.
                    let (start_frame, interleaved, ch) = if s.is_spectrum() {
                        let (v, ch) = s.snapshot();
                        (None, v, ch)
                    } else {
                        let (start, v, ch) = s.drain();
                        (Some(start), v, ch)
                    };
                    let frames = interleaved.len() / ch;
                    if frames == 0 {
                        continue;
                    }
                    let mut chans: Vec<Vec<f32>> = vec![Vec::with_capacity(frames); ch];
                    for f in 0..frames {
                        let base = f * ch;
                        for c in 0..ch {
                            chans[c].push(interleaved[base + c]);
                        }
                    }
                    let payload = match start_frame {
                        Some(start) => json!({
                            "nodeId": s.node_id,
                            "channels": ch,
                            "data": chans,
                            "sampleRate": s.sample_rate,
                            "startFrame": start,
                            "session": s.session,
                            "baseFrames": s.base_frames,
                        }),
                        None => json!({
                            "nodeId": s.node_id,
                            "channels": ch,
                            "data": chans,
                            "sampleRate": s.sample_rate,
                        }),
                    };
                    host.emit(SCOPE_EVENT, payload);
                }
            }
        })
        .expect("spawn meter tick thread");
    MeterTickThread {
        stop,
        join: Some(join),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_rate_tolerance_and_quantum() {
        // Healthy rate: within the relative tolerance.
        assert!(!off_rate(100.0, 102.0, 1.0));
        assert!(!off_rate(98.0, 100.0, 1.0));
        // Drift beyond 2% is flagged.
        assert!(off_rate(100.0, 110.0, 1.0));
        assert!(off_rate(100.0, 90.0, 1.0));
        // Zero expectation never flags.
        assert!(!off_rate(50.0, 0.0, 1.0));
        // A window-boundary misattribution is absorbed by the counter's step
        // size: at 1024-frame blocks, one block is 2.1% — bigger than the
        // 2% relative bound but within the 1.5× quantum slack.
        assert!(
            !off_rate(1024.0, 0.0 + 0.001, 1024.0),
            "one block's step must not flag"
        );
        assert!(!off_rate(0.0, 0.001, 1024.0));
        assert!(off_rate(0.0, 48_000.0, 1024.0), "dead output flags");
    }
}
