use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::warn;

use crate::audio::effects::{GrHandle, LufsHandle, MeterHandle, WaveformHandle};
use crate::audio::health;

use super::dag::{OutputMeta, SourceMeta, DSP_BLOCK_FRAMES};
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
    speaker_outputs: i64,
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
            let mut last_read: Vec<u64> = outputs
                .iter()
                .map(|o| {
                    o.io.as_ref()
                        .map_or(0, |io| io.read.load(Ordering::Relaxed))
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
                thread::sleep(XRUN_TICK);
                let now = Instant::now();
                let elapsed_secs = now.duration_since(last_tick).as_secs_f64();
                last_tick = now;

                for (i, s) in sources.iter().enumerate() {
                    let xrun_now = s.stats.xrun.load(Ordering::Relaxed);
                    let stalled_now = s.stats.stalled.load(Ordering::Relaxed);
                    let trimmed_now = s.stats.trimmed.load(Ordering::Relaxed);
                    let consumed_now = s.stats.consumed.load(Ordering::Relaxed);
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

                    let consumed_frames = consumed_delta / s.channels.max(1) as u64;
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
                        && (xrun_delta > 0
                            || stalled_delta > 0
                            || trimmed_delta > 0
                            || off_rate
                            || dropped_delta > 0)
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
                        o.sample_rate as f64 / DSP_BLOCK_FRAMES as f64 * elapsed_secs;
                    let blocks_off_rate = off_rate(blocks_delta as f64, expected_blocks, 1.0);

                    // Device-pull diagnostics: distinguishes "device asking for
                    // far more than real time implies" from "ring nobody fills",
                    // which the global OUTPUT_UNDERRUN_SAMPLES counter can't tell
                    // apart since it's summed across every output.
                    let io = o.io.as_ref().map(|io| {
                        let requested_now = io.requested.load(Ordering::Relaxed);
                        let read_now = io.read.load(Ordering::Relaxed);
                        let callbacks_now = io.callbacks.load(Ordering::Relaxed);
                        let requested_delta = requested_now.saturating_sub(last_requested[i]);
                        let read_delta = read_now.saturating_sub(last_read[i]);
                        let callbacks_delta = callbacks_now.saturating_sub(last_callbacks[i]);
                        last_requested[i] = requested_now;
                        last_read[i] = read_now;
                        last_callbacks[i] = callbacks_now;
                        (requested_delta, read_delta, callbacks_delta)
                    });
                    let io_off_rate = io.is_some_and(|(requested_delta, _, callbacks_delta)| {
                        let expected_samples = o.io.as_ref().map_or(o.sample_rate, |speaker| {
                            speaker.sample_rate.load(Ordering::Relaxed)
                        }) as f64
                            * o.channels as f64
                            * elapsed_secs;
                        // The device's own buffer size, measured rather than
                        // assumed: cpal opens with `BufferSize::Default`.
                        let quantum = if callbacks_delta > 0 {
                            requested_delta as f64 / callbacks_delta as f64
                        } else {
                            0.0
                        };
                        off_rate(requested_delta as f64, expected_samples, quantum)
                    });

                    if !warmup && (blocks_off_rate || io_off_rate) {
                        match io {
                            Some((requested_samples, read_samples, callbacks)) => warn!(
                                output = %o.label,
                                blocks = blocks_delta,
                                expected_blocks = expected_blocks.round() as u64,
                                requested_samples,
                                read_samples,
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

                // A stream that outlived its worker keeps calling back and
                // draining a ring nobody fills, which shows up in the global
                // underrun total but in no output's own counters.
                let live_streams = LIVE_SPEAKER_STREAMS.load(Ordering::Relaxed);
                if live_streams != speaker_outputs {
                    warn!(
                        live_speaker_streams = live_streams,
                        speaker_outputs, "orphan speaker streams"
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
    app: AppHandle,
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
                    let _ = app.emit(
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
                    let _ = app.emit(
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
                    let _ = app.emit(GR_EVENT, json!({ "nodeId": g.node_id, "grLin": gr_lin }));
                }
                for s in &scopes {
                    let (interleaved, ch) = s.snapshot();
                    let frames = interleaved.len() / ch;
                    let mut chans: Vec<Vec<f32>> = vec![Vec::with_capacity(frames); ch];
                    for f in 0..frames {
                        let base = f * ch;
                        for c in 0..ch {
                            chans[c].push(interleaved[base + c]);
                        }
                    }
                    let _ = app.emit(
                        SCOPE_EVENT,
                        json!({
                            "nodeId": s.node_id,
                            "channels": ch,
                            "data": chans,
                            "sampleRate": s.sample_rate,
                        }),
                    );
                }
            }
        })
        .expect("spawn meter tick thread");
    MeterTickThread {
        stop,
        join: Some(join),
    }
}
