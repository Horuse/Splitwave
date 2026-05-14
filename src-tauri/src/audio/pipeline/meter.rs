use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::audio::effects::{LufsHandle, MeterHandle};

const METER_EVENT: &str = "audio://meter";
const LUFS_EVENT: &str = "audio://lufs";
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
            }
        })
        .expect("spawn meter tick thread");
    MeterTickThread {
        stop,
        join: Some(join),
    }
}
