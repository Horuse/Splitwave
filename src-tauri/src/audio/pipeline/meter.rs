use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::audio::effects::{GrHandle, LufsHandle, MeterHandle, WaveformHandle};

const METER_EVENT: &str = "audio://meter";
const LUFS_EVENT: &str = "audio://lufs";
const GR_EVENT: &str = "audio://gr";
const SCOPE_EVENT: &str = "audio://scope";
const METER_TICK: Duration = Duration::from_millis(33);

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
                            "peakL": snap.peak_l,
                            "peakR": snap.peak_r,
                            "rmsL": snap.rms_l,
                            "rmsR": snap.rms_r,
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
                        }),
                    );
                }
                for g in &gr_handles {
                    let gr_lin = f32::from_bits(g.gr_lin.load(std::sync::atomic::Ordering::Relaxed));
                    let _ = app.emit(GR_EVENT, json!({ "nodeId": g.node_id, "grLin": gr_lin }));
                }
                for s in &scopes {
                    let interleaved = s.snapshot();
                    let l: Vec<f32> = interleaved.chunks_exact(2).map(|f| f[0]).collect();
                    let r: Vec<f32> = interleaved.chunks_exact(2).map(|f| f[1]).collect();
                    let _ = app.emit(SCOPE_EVENT, json!({ "nodeId": s.node_id, "l": l, "r": r }));
                }
            }
        })
        .expect("spawn meter tick thread");
    MeterTickThread {
        stop,
        join: Some(join),
    }
}
