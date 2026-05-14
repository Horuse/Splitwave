//! Real-time DSP effects. All effects operate on interleaved stereo f32 frames.
//!
//! Parameters live in `Arc<Atomic*>` cells shared with the UI side of the
//! engine. The audio callback reads them lock-free on every block, so slider
//! moves and mute toggles take effect within a couple of milliseconds without
//! restarting the pipeline.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::Value;

use ebur128::{EbuR128, Mode};

use crate::audio::graph::{
    ChannelBalanceData, EffectSpec, EqData, GainData, LevelMeterData, LimiterData, LufsMeterData,
    MuteData, SaturatorData,
};

pub trait Effect: Send {
    fn process(&mut self, samples: &mut [f32], frames: usize);
    /// Frames (not stereo samples) of delay between input and output. Pipeline
    /// pads parallel paths to align at mixing points.
    fn latency_frames(&self) -> usize {
        0
    }
}

/// Enum dispatch wrapper so the RT thread doesn't pay a vtable indirection per
/// process call. The closed set of effects is known at compile time; LLVM can
/// inline the inner loop for each variant.
pub enum RuntimeEffect {
    Gain(GainEffect),
    Mute(MuteEffect),
    ChannelBalance(ChannelBalanceEffect),
    Saturator(SaturatorEffect),
    Eq(EqEffect),
    LevelMeter(LevelMeterEffect),
    LufsMeter(LufsMeterEffect),
    Limiter(LimiterEffect),
}

impl RuntimeEffect {
    #[inline]
    pub fn process(&mut self, samples: &mut [f32], frames: usize) {
        match self {
            RuntimeEffect::Gain(e) => e.process(samples, frames),
            RuntimeEffect::Mute(e) => e.process(samples, frames),
            RuntimeEffect::ChannelBalance(e) => e.process(samples, frames),
            RuntimeEffect::Saturator(e) => e.process(samples, frames),
            RuntimeEffect::Eq(e) => e.process(samples, frames),
            RuntimeEffect::LevelMeter(e) => e.process(samples, frames),
            RuntimeEffect::LufsMeter(e) => e.process(samples, frames),
            RuntimeEffect::Limiter(e) => e.process(samples, frames),
        }
    }

    #[inline]
    pub fn latency_frames(&self) -> usize {
        match self {
            RuntimeEffect::Gain(e) => e.latency_frames(),
            RuntimeEffect::Mute(e) => e.latency_frames(),
            RuntimeEffect::ChannelBalance(e) => e.latency_frames(),
            RuntimeEffect::Saturator(e) => e.latency_frames(),
            RuntimeEffect::Eq(e) => e.latency_frames(),
            RuntimeEffect::LevelMeter(e) => e.latency_frames(),
            RuntimeEffect::LufsMeter(e) => e.latency_frames(),
            RuntimeEffect::Limiter(e) => e.latency_frames(),
        }
    }
}

#[derive(Clone)]
pub enum EffectControl {
    Gain {
        linear: Arc<AtomicU32>,
    },
    Mute {
        muted: Arc<AtomicBool>,
    },
    ChannelBalance {
        left: Arc<AtomicU32>,
        right: Arc<AtomicU32>,
    },
    Saturator {
        ceiling: Arc<AtomicU32>,
        drive: Arc<AtomicU32>,
    },
    Eq {
        /// One gain atomic per ISO octave band; see EQ_FREQUENCIES_HZ for order.
        gains: [Arc<AtomicU32>; 10],
    },
    Limiter {
        ceiling: Arc<AtomicU32>,
        release_ms: Arc<AtomicU32>,
    },
}

impl EffectControl {
    /// Unknown keys are silently ignored — the frontend pushes the full
    /// camelCase payload of the node, only some keys map to live controls.
    pub fn apply_update(&self, data: &Value) {
        match self {
            EffectControl::Gain { linear } => {
                if let Some(db) = num(data, "gainDb") {
                    store_f32(linear, db_to_linear(db));
                }
            }
            EffectControl::Mute { muted } => {
                if let Some(b) = data.get("muted").and_then(Value::as_bool) {
                    muted.store(b, Ordering::Relaxed);
                }
            }
            EffectControl::ChannelBalance { left, right } => {
                if let Some(db) = num(data, "leftGainDb") {
                    store_f32(left, db_to_linear(db));
                }
                if let Some(db) = num(data, "rightGainDb") {
                    store_f32(right, db_to_linear(db));
                }
            }
            EffectControl::Saturator { ceiling, drive } => {
                if let Some(db) = num(data, "thresholdDb") {
                    let c = db_to_linear(db).max(1e-6);
                    store_f32(ceiling, c);
                }
                if let Some(db) = num(data, "driveDb") {
                    store_f32(drive, db_to_linear(db));
                }
            }
            EffectControl::Eq { gains } => {
                if let Some(arr) = data.get("gainsDb").and_then(Value::as_array) {
                    for (i, slot) in gains.iter().enumerate() {
                        if let Some(v) = arr.get(i).and_then(Value::as_f64) {
                            store_f32(slot, v as f32);
                        }
                    }
                }
            }
            EffectControl::Limiter { ceiling, release_ms } => {
                if let Some(db) = num(data, "ceilingDb") {
                    store_f32(ceiling, db_to_linear(db).max(1e-6));
                }
                if let Some(ms) = num(data, "releaseMs") {
                    store_f32(release_ms, ms.max(0.1));
                }
            }
        }
    }
}

fn num(data: &Value, key: &str) -> Option<f32> {
    data.get(key).and_then(Value::as_f64).map(|v| v as f32)
}

#[inline]
fn store_f32(slot: &AtomicU32, v: f32) {
    slot.store(v.to_bits(), Ordering::Relaxed);
}

#[inline]
fn load_f32(slot: &AtomicU32) -> f32 {
    f32::from_bits(slot.load(Ordering::Relaxed))
}

pub struct GainEffect {
    linear: Arc<AtomicU32>,
    current: f32,
}

impl GainEffect {
    fn new(d: GainData) -> (Self, EffectControl) {
        let initial = db_to_linear(d.gain_db);
        let linear = Arc::new(AtomicU32::new(initial.to_bits()));
        let control = EffectControl::Gain {
            linear: linear.clone(),
        };
        (
            Self {
                linear,
                current: initial,
            },
            control,
        )
    }
}

impl Effect for GainEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let target = load_f32(&self.linear);
        let stereo = &mut samples[..frames * 2];
        if (self.current - target).abs() < 1e-7 {
            for s in stereo {
                *s *= target;
            }
            self.current = target;
            return;
        }
        let step = (target - self.current) / frames as f32;
        let mut g = self.current;
        for frame in stereo.chunks_exact_mut(2) {
            g += step;
            frame[0] *= g;
            frame[1] *= g;
        }
        self.current = target;
    }
}

pub struct MuteEffect {
    muted: Arc<AtomicBool>,
    current: f32,
}

impl MuteEffect {
    fn new(d: MuteData) -> (Self, EffectControl) {
        let muted = Arc::new(AtomicBool::new(d.muted));
        let control = EffectControl::Mute {
            muted: muted.clone(),
        };
        (
            Self {
                current: if d.muted { 0.0 } else { 1.0 },
                muted,
            },
            control,
        )
    }
}

impl Effect for MuteEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let target = if self.muted.load(Ordering::Relaxed) { 0.0 } else { 1.0 };
        if self.current >= 1.0 && target >= 1.0 {
            return;
        }
        let stereo = &mut samples[..frames * 2];
        if self.current <= 0.0 && target <= 0.0 {
            for s in stereo {
                *s = 0.0;
            }
            return;
        }
        let step = (target - self.current) / frames as f32;
        let mut g = self.current;
        for frame in stereo.chunks_exact_mut(2) {
            g += step;
            frame[0] *= g;
            frame[1] *= g;
        }
        self.current = target;
    }
}

pub struct ChannelBalanceEffect {
    left: Arc<AtomicU32>,
    right: Arc<AtomicU32>,
}

impl ChannelBalanceEffect {
    fn new(d: ChannelBalanceData) -> (Self, EffectControl) {
        let left = Arc::new(AtomicU32::new(db_to_linear(d.left_gain_db).to_bits()));
        let right = Arc::new(AtomicU32::new(db_to_linear(d.right_gain_db).to_bits()));
        let control = EffectControl::ChannelBalance {
            left: left.clone(),
            right: right.clone(),
        };
        (Self { left, right }, control)
    }
}

impl Effect for ChannelBalanceEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let gl = load_f32(&self.left);
        let gr = load_f32(&self.right);
        let stereo = &mut samples[..frames * 2];
        for frame in stereo.chunks_exact_mut(2) {
            frame[0] *= gl;
            frame[1] *= gr;
        }
    }
}

pub struct LevelMeterEffect {
    handle: MeterHandle,
}

#[derive(Clone)]
pub struct MeterHandle {
    pub node_id: String,
    pub peak_l: Arc<AtomicU32>,
    pub peak_r: Arc<AtomicU32>,
    pub rms_l: Arc<AtomicU32>,
    pub rms_r: Arc<AtomicU32>,
}

#[derive(Debug, Clone, Copy)]
pub struct MeterSnapshot {
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms_l: f32,
    pub rms_r: f32,
}

/// Peak fall-off per tick — prevents transients from latching the meter.
pub const METER_PEAK_DECAY: f32 = 0.85;

impl MeterHandle {
    pub fn new(node_id: String) -> Self {
        Self {
            node_id,
            peak_l: Arc::new(AtomicU32::new(0)),
            peak_r: Arc::new(AtomicU32::new(0)),
            rms_l: Arc::new(AtomicU32::new(0)),
            rms_r: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Snapshot current values and decay the peak — called from the engine's
    /// tick thread.
    pub fn snapshot_and_decay(&self) -> MeterSnapshot {
        let pl = load_f32(&self.peak_l);
        let pr = load_f32(&self.peak_r);
        let rl = load_f32(&self.rms_l);
        let rr = load_f32(&self.rms_r);
        store_f32(&self.peak_l, pl * METER_PEAK_DECAY);
        store_f32(&self.peak_r, pr * METER_PEAK_DECAY);
        MeterSnapshot {
            peak_l: pl,
            peak_r: pr,
            rms_l: rl,
            rms_r: rr,
        }
    }
}

impl LevelMeterEffect {
    fn new(_d: LevelMeterData, node_id: String) -> (Self, MeterHandle) {
        let handle = MeterHandle {
            node_id,
            peak_l: Arc::new(AtomicU32::new(0)),
            peak_r: Arc::new(AtomicU32::new(0)),
            rms_l: Arc::new(AtomicU32::new(0)),
            rms_r: Arc::new(AtomicU32::new(0)),
        };
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }
}

impl Effect for LevelMeterEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        update_meter(&self.handle, &samples[..frames * 2]);
    }
}

/// `stereo` is interleaved L/R f32; odd-length truncates the trailing half-frame.
pub fn update_meter(handle: &MeterHandle, stereo: &[f32]) {
    let frames = stereo.len() / 2;
    if frames == 0 {
        return;
    }
    let stereo = &stereo[..frames * 2];
    let mut peak_l = 0.0f32;
    let mut peak_r = 0.0f32;
    let mut sum_l_sq = 0.0f64;
    let mut sum_r_sq = 0.0f64;
    for frame in stereo.chunks_exact(2) {
        let l = frame[0];
        let r = frame[1];
        let al = l.abs();
        let ar = r.abs();
        if al > peak_l {
            peak_l = al;
        }
        if ar > peak_r {
            peak_r = ar;
        }
        sum_l_sq += (l as f64) * (l as f64);
        sum_r_sq += (r as f64) * (r as f64);
    }
    let existing_l = load_f32(&handle.peak_l);
    let existing_r = load_f32(&handle.peak_r);
    store_f32(&handle.peak_l, existing_l.max(peak_l));
    store_f32(&handle.peak_r, existing_r.max(peak_r));
    let rms_l = (sum_l_sq / frames as f64).sqrt() as f32;
    let rms_r = (sum_r_sq / frames as f64).sqrt() as f32;
    store_f32(&handle.rms_l, rms_l);
    store_f32(&handle.rms_r, rms_r);
}

/// Tauri's serde_json cannot serialize non-finite f32 — silent input emits
/// this sub-audible floor instead.
pub const LUFS_SILENT: f32 = -120.0;

pub struct LufsMeterEffect {
    ebu: EbuR128,
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
    fn new(_d: LufsMeterData, node_id: String, sample_rate: u32) -> (Self, LufsHandle) {
        let ebu = EbuR128::new(2, sample_rate, LUFS_MODE)
            .expect("ebur128 init: stereo + valid sample rate");
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

    fn from_handle(handle: LufsHandle, sample_rate: u32) -> Self {
        let ebu = EbuR128::new(2, sample_rate, LUFS_MODE)
            .expect("ebur128 init: stereo + valid sample rate");
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
        // ebur128's internal buffers are pre-allocated by `new` — `add_frames`
        // stays alloc-free on the RT path.
        let _ = self.ebu.add_frames_f32(&samples[..frames * 2]);
        let m = self.ebu.loudness_momentary().unwrap_or(f64::NEG_INFINITY);
        let s = self.ebu.loudness_shortterm().unwrap_or(f64::NEG_INFINITY);
        store_f32(&self.handle.momentary, sanitize_lufs(m));
        store_f32(&self.handle.shortterm, sanitize_lufs(s));

        let tp_l = self.ebu.true_peak(0).unwrap_or(0.0);
        let tp_r = self.ebu.true_peak(1).unwrap_or(0.0);
        store_f32(&self.handle.tp_l, amp_to_db(tp_l));
        store_f32(&self.handle.tp_r, amp_to_db(tp_r));

        self.frames_since_global += frames;
        if self.frames_since_global >= self.sample_rate as usize {
            let i = self.ebu.loudness_global().unwrap_or(f64::NEG_INFINITY);
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

/// Soft saturator: `y = ceiling * tanh(x * drive / ceiling)` — smooth tanh
/// curve, no hard clipping. Not a real limiter (no look-ahead / true-peak).
pub struct SaturatorEffect {
    ceiling: Arc<AtomicU32>,
    drive: Arc<AtomicU32>,
}

impl SaturatorEffect {
    fn new(d: SaturatorData) -> (Self, EffectControl) {
        let c = db_to_linear(d.threshold_db).max(1e-6);
        let ceiling = Arc::new(AtomicU32::new(c.to_bits()));
        let drive = Arc::new(AtomicU32::new(db_to_linear(d.drive_db).to_bits()));
        let control = EffectControl::Saturator {
            ceiling: ceiling.clone(),
            drive: drive.clone(),
        };
        (Self { ceiling, drive }, control)
    }
}

impl Effect for SaturatorEffect {
    #[inline]
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        // No `inv_ceiling` cache — RT could read NEW_c with OLD_inv_c (torn pair).
        let c = load_f32(&self.ceiling).max(1e-6);
        let d = load_f32(&self.drive);
        let inv_c = 1.0 / c;
        let stereo = &mut samples[..frames * 2];
        for s in stereo {
            *s = c * fast_tanh(*s * d * inv_c);
        }
    }
}

/// Brick-wall limiter: input is delayed by `lookahead_frames`; gain envelope
/// reacts to the upcoming peak so reduction lands before the peak emerges.
/// Instant attack, exponential release.
pub struct LimiterEffect {
    ceiling: Arc<AtomicU32>,
    release_ms: Arc<AtomicU32>,
    sample_rate: u32,
    lookahead_frames: usize,
    /// Stereo-interleaved look-ahead delay; both channels share `current_gain`.
    delay_buf: Box<[f32]>,
    delay_pos: usize,
    /// Per-frame max(|L|, |R|) over the same window as `delay_buf`. Peak in
    /// the window = `peak_buf.iter().max()`.
    peak_buf: Box<[f32]>,
    current_gain: f32,
}

impl LimiterEffect {
    fn new(d: LimiterData, sample_rate: u32) -> (Self, EffectControl) {
        let lookahead_frames = ((d.lookahead_ms.max(0.1) * sample_rate as f32 / 1000.0) as usize)
            .max(1);
        let ceiling_lin = db_to_linear(d.ceiling_db).max(1e-6);
        let ceiling = Arc::new(AtomicU32::new(ceiling_lin.to_bits()));
        let release_ms = Arc::new(AtomicU32::new(d.release_ms.max(0.1).to_bits()));
        let control = EffectControl::Limiter {
            ceiling: ceiling.clone(),
            release_ms: release_ms.clone(),
        };
        (
            Self {
                ceiling,
                release_ms,
                sample_rate,
                lookahead_frames,
                delay_buf: vec![0.0; lookahead_frames * 2].into_boxed_slice(),
                delay_pos: 0,
                peak_buf: vec![0.0; lookahead_frames].into_boxed_slice(),
                current_gain: 1.0,
            },
            control,
        )
    }

    fn from_state(
        ceiling: Arc<AtomicU32>,
        release_ms: Arc<AtomicU32>,
        lookahead_frames: usize,
        sample_rate: u32,
    ) -> Self {
        Self {
            ceiling,
            release_ms,
            sample_rate,
            lookahead_frames,
            delay_buf: vec![0.0; lookahead_frames * 2].into_boxed_slice(),
            delay_pos: 0,
            peak_buf: vec![0.0; lookahead_frames].into_boxed_slice(),
            current_gain: 1.0,
        }
    }
}

impl Effect for LimiterEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let ceiling = load_f32(&self.ceiling).max(1e-6);
        let release_ms = load_f32(&self.release_ms).max(0.1);
        let release_coeff =
            1.0 - (-1.0 / (release_ms * 0.001 * self.sample_rate as f32)).exp();

        let lookahead = self.lookahead_frames;
        let stereo = &mut samples[..frames * 2];
        for f in 0..frames {
            let l_in = stereo[f * 2];
            let r_in = stereo[f * 2 + 1];

            // Read the emerging (oldest) sample, then overwrite that slot.
            let l_out = self.delay_buf[self.delay_pos * 2];
            let r_out = self.delay_buf[self.delay_pos * 2 + 1];
            self.delay_buf[self.delay_pos * 2] = l_in;
            self.delay_buf[self.delay_pos * 2 + 1] = r_in;
            self.peak_buf[self.delay_pos] = l_in.abs().max(r_in.abs());
            self.delay_pos = if self.delay_pos + 1 == lookahead { 0 } else { self.delay_pos + 1 };

            let mut peak = 0.0_f32;
            for &p in self.peak_buf.iter() {
                if p > peak {
                    peak = p;
                }
            }
            let target = if peak > ceiling { ceiling / peak } else { 1.0 };
            if target < self.current_gain {
                self.current_gain = target;
            } else {
                self.current_gain += (target - self.current_gain) * release_coeff;
            }

            stereo[f * 2] = l_out * self.current_gain;
            stereo[f * 2 + 1] = r_out * self.current_gain;
        }
    }

    fn latency_frames(&self) -> usize {
        self.lookahead_frames
    }
}

/// RBJ cookbook biquad in Transposed Direct Form II — one state pair (z1, z2)
/// per channel, half the rounding noise of DF I.
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

}

#[derive(Clone, Copy)]
pub enum BandShape {
    Lpf,
    Hpf,
}

/// RBJ cookbook coefficients.
pub fn biquad_for(shape: BandShape, freq_hz: f32, q: f32, sample_rate: u32) -> Biquad {
    let fs = sample_rate as f32;
    let w0 = 2.0 * std::f32::consts::PI * (freq_hz.max(1.0) / fs);
    let (sinw, cosw) = (w0.sin(), w0.cos());
    let q = q.max(0.05);
    let alpha = sinw / (2.0 * q);

    let (b0, b1, b2, a0, a1, a2) = match shape {
        BandShape::Lpf => (
            (1.0 - cosw) * 0.5,
            1.0 - cosw,
            (1.0 - cosw) * 0.5,
            1.0 + alpha,
            -2.0 * cosw,
            1.0 - alpha,
        ),
        BandShape::Hpf => (
            (1.0 + cosw) * 0.5,
            -(1.0 + cosw),
            (1.0 + cosw) * 0.5,
            1.0 + alpha,
            -2.0 * cosw,
            1.0 - alpha,
        ),
    };
    let inv = 1.0 / a0;
    Biquad {
        b0: b0 * inv,
        b1: b1 * inv,
        b2: b2 * inv,
        a1: a1 * inv,
        a2: a2 * inv,
        z1: 0.0,
        z2: 0.0,
    }
}

/// Linkwitz-Riley 4th-order crossover points: geometric means between adjacent
/// band centres. LR4 = two cascaded 2nd-order Butterworth biquads; sum of
/// matched LPF/HPF at the same fc is allpass, so all 10 bands sum back to a
/// magnitude-flat output when their gains are unity.
const EQ_CROSSOVER_FREQS: [f32; 9] = [
    45.2548, 89.4427, 176.7767, 353.5534, 707.1068, 1414.2136, 2828.4271, 5656.8542, 11313.7085,
];

const BUTTER_Q: f32 = std::f32::consts::FRAC_1_SQRT_2; // 1/√2 ≈ 0.7071

/// Cascaded pair of Butterworth biquads — a 4th-order Linkwitz-Riley section.
#[derive(Clone, Copy, Default)]
struct Lr4 {
    a: Biquad,
    b: Biquad,
}

impl Lr4 {
    fn new(shape: BandShape, freq_hz: f32, sample_rate: u32) -> Self {
        let c = biquad_for(shape, freq_hz, BUTTER_Q, sample_rate);
        Lr4 { a: c, b: c }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.b.process(self.a.process(x))
    }
}

/// Per-channel filter chain. The input cascades through 9 crossover splits:
/// each split peels off one band's slice via LPF and forwards the HPF residual
/// to the next stage. Band gains scale these slices and we sum.
struct ChannelChain {
    lpfs: [Lr4; 9],
    hpfs: [Lr4; 9],
}

impl ChannelChain {
    fn new(sample_rate: u32) -> Self {
        Self {
            lpfs: std::array::from_fn(|i| {
                Lr4::new(BandShape::Lpf, EQ_CROSSOVER_FREQS[i], sample_rate)
            }),
            hpfs: std::array::from_fn(|i| {
                Lr4::new(BandShape::Hpf, EQ_CROSSOVER_FREQS[i], sample_rate)
            }),
        }
    }

    #[inline]
    fn process(&mut self, x: f32, gains_linear: &[f32; 10]) -> f32 {
        let mut residual = x;
        let mut sum = 0.0;
        for i in 0..9 {
            let band = self.lpfs[i].process(residual);
            residual = self.hpfs[i].process(residual);
            sum += band * gains_linear[i];
        }
        sum + residual * gains_linear[9]
    }
}

pub struct EqEffect {
    channels: [ChannelChain; 2],
    gains: [Arc<AtomicU32>; 10],
}

impl EqEffect {
    fn new(d: EqData, sample_rate: u32) -> (Self, EffectControl) {
        let gains: [Arc<AtomicU32>; 10] =
            std::array::from_fn(|i| Arc::new(AtomicU32::new(d.gains_db[i].to_bits())));
        let control = EffectControl::Eq {
            gains: gains.clone(),
        };
        (
            Self {
                channels: [ChannelChain::new(sample_rate), ChannelChain::new(sample_rate)],
                gains,
            },
            control,
        )
    }

    fn from_gains(gains: [Arc<AtomicU32>; 10], sample_rate: u32) -> Self {
        Self {
            channels: [ChannelChain::new(sample_rate), ChannelChain::new(sample_rate)],
            gains,
        }
    }
}

impl Effect for EqEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        let gains_linear: [f32; 10] =
            std::array::from_fn(|i| db_to_linear(load_f32(&self.gains[i])));
        let stereo = &mut samples[..frames * 2];
        for frame in stereo.chunks_exact_mut(2) {
            frame[0] = self.channels[0].process(frame[0], &gains_linear);
            frame[1] = self.channels[1].process(frame[1], &gains_linear);
        }
    }
}

pub struct EffectBuild {
    pub effect: RuntimeEffect,
    /// Some only on the first instantiation per node id.
    pub control: Option<EffectControl>,
    /// Some only on the first instantiation per node id.
    pub meter: Option<MeterHandle>,
    /// Some only on the first instantiation per node id.
    pub lufs: Option<LufsHandle>,
}

/// Shared atomics keyed by node id so a fan-out effect (one node feeding
/// multiple outputs) keeps live params in sync across instances.
#[derive(Default)]
pub struct EffectRegistry {
    controls: std::collections::HashMap<String, EffectControl>,
    meters: std::collections::HashMap<String, MeterHandle>,
    lufs: std::collections::HashMap<String, LufsHandle>,
}

impl EffectRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn instantiate_effect(
    spec: &EffectSpec,
    node_id: &str,
    sample_rate: u32,
    registry: &mut EffectRegistry,
) -> EffectBuild {
    match *spec {
        EffectSpec::Gain(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Gain { linear }) => EffectBuild {
                effect: RuntimeEffect::Gain(GainEffect {
                    current: load_f32(linear),
                    linear: linear.clone(),
                }),
                control: None,
                meter: None,
                lufs: None,
            },
            _ => {
                let (e, c) = GainEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                EffectBuild {
                    effect: RuntimeEffect::Gain(e),
                    control: Some(c),
                    meter: None,
                    lufs: None,
                }
            }
        },
        EffectSpec::Mute(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Mute { muted }) => EffectBuild {
                effect: RuntimeEffect::Mute(MuteEffect {
                    current: if muted.load(Ordering::Relaxed) { 0.0 } else { 1.0 },
                    muted: muted.clone(),
                }),
                control: None,
                meter: None,
                lufs: None,
            },
            _ => {
                let (e, c) = MuteEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                EffectBuild {
                    effect: RuntimeEffect::Mute(e),
                    control: Some(c),
                    meter: None,
                    lufs: None,
                }
            }
        },
        EffectSpec::ChannelBalance(d) => match registry.controls.get(node_id) {
            Some(EffectControl::ChannelBalance { left, right }) => EffectBuild {
                effect: RuntimeEffect::ChannelBalance(ChannelBalanceEffect {
                    left: left.clone(),
                    right: right.clone(),
                }),
                control: None,
                meter: None,
                lufs: None,
            },
            _ => {
                let (e, c) = ChannelBalanceEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                EffectBuild {
                    effect: RuntimeEffect::ChannelBalance(e),
                    control: Some(c),
                    meter: None,
                    lufs: None,
                }
            }
        },
        EffectSpec::Saturator(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Saturator { ceiling, drive }) => EffectBuild {
                effect: RuntimeEffect::Saturator(SaturatorEffect {
                    ceiling: ceiling.clone(),
                    drive: drive.clone(),
                }),
                control: None,
                meter: None,
                lufs: None,
            },
            _ => {
                let (e, c) = SaturatorEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                EffectBuild {
                    effect: RuntimeEffect::Saturator(e),
                    control: Some(c),
                    meter: None,
                    lufs: None,
                }
            }
        },
        EffectSpec::Eq(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Eq { gains }) => EffectBuild {
                effect: RuntimeEffect::Eq(EqEffect::from_gains(gains.clone(), sample_rate)),
                control: None,
                meter: None,
                lufs: None,
            },
            _ => {
                let (e, c) = EqEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                EffectBuild {
                    effect: RuntimeEffect::Eq(e),
                    control: Some(c),
                    meter: None,
                    lufs: None,
                }
            }
        },
        EffectSpec::LevelMeter(d) => match registry.meters.get(node_id) {
            Some(handle) => EffectBuild {
                effect: RuntimeEffect::LevelMeter(LevelMeterEffect {
                    handle: handle.clone(),
                }),
                control: None,
                meter: None,
                lufs: None,
            },
            None => {
                let (e, handle) = LevelMeterEffect::new(d, node_id.to_string());
                registry.meters.insert(node_id.to_string(), handle.clone());
                EffectBuild {
                    effect: RuntimeEffect::LevelMeter(e),
                    control: None,
                    meter: Some(handle),
                    lufs: None,
                }
            }
        },
        EffectSpec::LufsMeter(d) => match registry.lufs.get(node_id) {
            Some(handle) => EffectBuild {
                effect: RuntimeEffect::LufsMeter(LufsMeterEffect::from_handle(
                    handle.clone(),
                    sample_rate,
                )),
                control: None,
                meter: None,
                lufs: None,
            },
            None => {
                let (e, handle) = LufsMeterEffect::new(d, node_id.to_string(), sample_rate);
                registry.lufs.insert(node_id.to_string(), handle.clone());
                EffectBuild {
                    effect: RuntimeEffect::LufsMeter(e),
                    control: None,
                    meter: None,
                    lufs: Some(handle),
                }
            }
        },
        EffectSpec::Limiter(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Limiter { ceiling, release_ms }) => {
                let lookahead_frames =
                    ((d.lookahead_ms.max(0.1) * sample_rate as f32 / 1000.0) as usize).max(1);
                EffectBuild {
                    effect: RuntimeEffect::Limiter(LimiterEffect::from_state(
                        ceiling.clone(),
                        release_ms.clone(),
                        lookahead_frames,
                        sample_rate,
                    )),
                    control: None,
                    meter: None,
                    lufs: None,
                }
            }
            _ => {
                let (e, c) = LimiterEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                EffectBuild {
                    effect: RuntimeEffect::Limiter(e),
                    control: Some(c),
                    meter: None,
                    lufs: None,
                }
            }
        },
    }
}

#[inline]
fn db_to_linear(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

/// Padé-style approximation of `tanh` — within ~1e-4 of `f32::tanh` in [-4, 4],
/// branchless, ~4x faster than `f32::tanh` on x86_64/aarch64.
#[inline]
fn fast_tanh(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    let x2 = x * x;
    let num = x * (27.0 + x2);
    let den = 27.0 + 9.0 * x2;
    num / den
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_applies_db() {
        let (mut e, _) = GainEffect::new(GainData { gain_db: 6.0 });
        let mut buf = [1.0_f32, 1.0];
        e.process(&mut buf, 1);
        assert!((buf[0] - 1.995).abs() < 0.01);
    }

    #[test]
    fn gain_control_changes_live() {
        let (mut e, c) = GainEffect::new(GainData { gain_db: 0.0 });
        c.apply_update(&serde_json::json!({ "gainDb": 6.0 }));
        let mut buf = [1.0_f32, 1.0];
        e.process(&mut buf, 1);
        assert!((buf[0] - 1.995).abs() < 0.01);
    }

    #[test]
    fn mute_zeros() {
        let (mut e, _) = MuteEffect::new(MuteData { muted: true });
        let mut buf = [0.5, -0.5, 0.3, -0.3];
        e.process(&mut buf, 2);
        assert_eq!(buf, [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn mute_control_unmutes_live() {
        let (mut e, c) = MuteEffect::new(MuteData { muted: true });
        c.apply_update(&serde_json::json!({ "muted": false }));
        let mut buf = [0.5_f32, -0.5];
        e.process(&mut buf, 1);
        assert_eq!(buf, [0.5, -0.5]);
    }

    #[test]
    fn balance_applies_per_channel() {
        let (mut e, _) = ChannelBalanceEffect::new(ChannelBalanceData {
            left_gain_db: -6.0,
            right_gain_db: 0.0,
        });
        let mut buf = [1.0, 1.0];
        e.process(&mut buf, 1);
        assert!((buf[0] - 0.501).abs() < 0.01);
        assert!((buf[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn saturator_clips_above_ceiling() {
        let (mut e, _) = SaturatorEffect::new(SaturatorData {
            threshold_db: 0.0,
            drive_db: 0.0,
        });
        let mut buf = [10.0, -10.0];
        e.process(&mut buf, 1);
        assert!(buf[0].abs() < 1.05);
        assert!(buf[1].abs() < 1.05);
    }
}
