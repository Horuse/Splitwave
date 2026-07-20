use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter};
use tracing::warn;

use crate::audio::effects::{GrHandle, LufsHandle, MeterHandle, WaveformHandle};

const METER_EVENT: &str = "audio://meter";
const LUFS_EVENT: &str = "audio://lufs";
const GR_EVENT: &str = "audio://gr";
const SCOPE_EVENT: &str = "audio://scope";
const METER_TICK: Duration = Duration::from_millis(33);

const XRUN_TICK: Duration = Duration::from_millis(1000);

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

/// Polls per-source underrun counters once a second and logs the delta. A
/// growing count means our DSP path starved (ring ran dry mid-block); silence
/// with a flat count points downstream to the device/driver instead.
pub(super) fn spawn_xrun_thread(handles: Vec<(String, Arc<AtomicU64>)>) -> XrunTickThread {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let join = thread::Builder::new()
        .name("xrun-tick".into())
        .spawn(move || {
            let mut last: Vec<u64> = vec![0; handles.len()];
            while !stop_thread.load(Ordering::SeqCst) {
                thread::sleep(XRUN_TICK);
                for (i, (label, counter)) in handles.iter().enumerate() {
                    let now = counter.load(Ordering::Relaxed);
                    let delta = now.saturating_sub(last[i]);
                    last[i] = now;
                    if delta > 0 {
                        warn!(source = %label, underrun_samples = delta, "DSP underrun");
                    }
                }
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
                    let gr_lin = f32::from_bits(g.gr_lin.load(std::sync::atomic::Ordering::Relaxed));
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
