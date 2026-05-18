use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use ebur128::{EbuR128, Mode};

use crate::audio::graph::LufsMeterData;

use super::util::{load_f32, store_f32};
use super::Effect;

/// Tauri's serde_json cannot serialize non-finite f32 — silent input emits
/// this sub-audible floor instead.
pub const LUFS_SILENT: f32 = -120.0;

pub struct LufsMeterEffect {
    ebu: Option<EbuR128>,
    handle: LufsHandle,
    /// `loudness_global` iterates the entire stored block history (O(N)),
    /// so we throttle it to ~once per second.
    frames_since_global: usize,
    sample_rate: u32,
}

#[derive(Clone)]
pub struct LufsHandle {
    pub node_id: String,
    pub momentary: Arc<AtomicU32>,
    pub shortterm: Arc<AtomicU32>,
    pub integrated: Arc<AtomicU32>,
    /// 4×-oversampled true peak (dBTP, per ITU-R BS.1770) — catches
    /// inter-sample peaks invisible to a sample-domain meter.
    pub tp_l: Arc<AtomicU32>,
    pub tp_r: Arc<AtomicU32>,
}

#[derive(Debug, Clone, Copy)]
pub struct LufsSnapshot {
    pub momentary: f32,
    pub shortterm: f32,
    pub integrated: f32,
    pub tp_l: f32,
    pub tp_r: f32,
}

impl LufsHandle {
    pub fn snapshot(&self) -> LufsSnapshot {
        LufsSnapshot {
            momentary: load_f32(&self.momentary),
            shortterm: load_f32(&self.shortterm),
            integrated: load_f32(&self.integrated),
            tp_l: load_f32(&self.tp_l),
            tp_r: load_f32(&self.tp_r),
        }
    }
}

const LUFS_MODE: Mode = Mode::I.union(Mode::M).union(Mode::S).union(Mode::TRUE_PEAK);

impl LufsMeterEffect {
    pub fn new(_d: LufsMeterData, node_id: String, sample_rate: u32) -> (Self, LufsHandle) {
        let ebu = EbuR128::new(2, sample_rate, LUFS_MODE).ok();
        let handle = LufsHandle {
            node_id,
            momentary: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            shortterm: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            integrated: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            tp_l: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            tp_r: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
        };
        (
            Self {
                ebu,
                handle: handle.clone(),
                frames_since_global: 0,
                sample_rate,
            },
            handle,
        )
    }

    pub fn from_handle(handle: LufsHandle, sample_rate: u32) -> Self {
        let ebu = EbuR128::new(2, sample_rate, LUFS_MODE).ok();
        Self {
            ebu,
            handle,
            frames_since_global: 0,
            sample_rate,
        }
    }
}

impl Effect for LufsMeterEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        let Some(ebu) = &mut self.ebu else { return };
        // ebur128's internal buffers are pre-allocated by `new` — `add_frames`
        // stays alloc-free on the RT path.
        let _ = ebu.add_frames_f32(&samples[..frames * 2]);
        let m = ebu.loudness_momentary().unwrap_or(f64::NEG_INFINITY);
        let s = ebu.loudness_shortterm().unwrap_or(f64::NEG_INFINITY);
        store_f32(&self.handle.momentary, sanitize_lufs(m));
        store_f32(&self.handle.shortterm, sanitize_lufs(s));

        let tp_l = ebu.true_peak(0).unwrap_or(0.0);
        let tp_r = ebu.true_peak(1).unwrap_or(0.0);
        store_f32(&self.handle.tp_l, amp_to_db(tp_l));
        store_f32(&self.handle.tp_r, amp_to_db(tp_r));

        self.frames_since_global += frames;
        if self.frames_since_global >= self.sample_rate as usize {
            let i = ebu.loudness_global().unwrap_or(f64::NEG_INFINITY);
            store_f32(&self.handle.integrated, sanitize_lufs(i));
            self.frames_since_global = 0;
        }
    }
}

#[inline]
fn sanitize_lufs(v: f64) -> f32 {
    let v = v as f32;
    if v.is_finite() {
        v.max(LUFS_SILENT)
    } else {
        LUFS_SILENT
    }
}

#[inline]
fn amp_to_db(amp: f64) -> f32 {
    let a = amp as f32;
    if a > 1e-6 {
        (20.0 * a.log10()).max(LUFS_SILENT)
    } else {
        LUFS_SILENT
    }
}
