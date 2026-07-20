use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use ebur128::{EbuR128, Mode};

use crate::audio::graph::LufsMeterData;

use super::util::{load_f32, store_f32};
use super::Effect;

/// Tauri's serde_json cannot serialize non-finite f32 — silent input emits
/// this sub-audible floor instead.
pub const LUFS_SILENT: f32 = -120.0;

/// Window for the noise-floor search. ACX judges the floor over silent
/// passages, so a window has to be long enough to sit inside one gap.
const NOISE_WINDOW_MS: usize = 300;

/// Sized for the DSP block; the analyser only ever sees two channels of it.
const SCRATCH_FRAMES: usize = 1024;

/// A sample at or past this counts as clipped; f32 can exceed 1.0, so the
/// test is on magnitude rather than equality.
const CLIP_LEVEL: f32 = 0.999;

/// Below this a window is digital black (nothing recorded yet, or a hard
/// mute) rather than room tone, and would peg the floor at -120 forever.
const NOISE_IGNORE_BELOW: f32 = -90.0;

pub struct LufsMeterEffect {
    /// One analyser per width: `EbuR128::new` allocates, so both are built at
    /// graph time and picked per block. A mono signal fed to a stereo analyser
    /// would read ~3 LU hot, and slicing it as stereo would panic outright.
    ebu: Option<EbuR128>,
    ebu_mono: Option<EbuR128>,
    /// First two channels of a wider block, deinterleaved for the analyser.
    scratch: Vec<f32>,
    handle: LufsHandle,
    /// `loudness_global` iterates the entire stored block history (O(N)),
    /// so we throttle it to ~once per second.
    frames_since_global: usize,
    sample_rate: u32,
    /// Unweighted, ungated running RMS. ACX predates BS.1770 and specifies
    /// plain RMS, which K-weighted LUFS cannot stand in for.
    sum_sq: f64,
    total_frames: u64,
    win_sum_sq: f64,
    win_frames: usize,
    window_frames: usize,
    noise_floor: f32,
    sum_dc: f64,
    sample_peak: f32,
    clips: u32,
    /// Correlation is a short-window reading, so it resets with the noise window.
    win_lr: f64,
    win_ll: f64,
    win_rr: f64,
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
    /// Loudness range (LU) per EBU R128 — statistical spread of short-term
    /// loudness across the program.
    pub lra: Arc<AtomicU32>,
    /// Unweighted RMS (dBFS) since the pipeline started, and the quietest
    /// sustained window seen — the two numbers ACX submission is judged on.
    pub rms: Arc<AtomicU32>,
    pub noise_floor: Arc<AtomicU32>,
    pub sample_peak: Arc<AtomicU32>,
    /// Mean sample value; a non-zero DC offset wastes headroom and can thump
    /// on edit boundaries.
    pub dc_offset: Arc<AtomicU32>,
    /// Stereo correlation in -1..1; below zero warns of mono-fold cancellation.
    pub correlation: Arc<AtomicU32>,
    /// Samples at or past full scale, counted since the pipeline started.
    pub clips: Arc<AtomicU32>,
}

#[derive(Debug, Clone, Copy)]
pub struct LufsSnapshot {
    pub momentary: f32,
    pub shortterm: f32,
    pub integrated: f32,
    pub tp_l: f32,
    pub tp_r: f32,
    pub lra: f32,
    pub rms: f32,
    pub noise_floor: f32,
    pub sample_peak: f32,
    pub dc_offset: f32,
    pub correlation: f32,
    pub clips: u32,
}

impl LufsHandle {
    pub fn snapshot(&self) -> LufsSnapshot {
        LufsSnapshot {
            momentary: load_f32(&self.momentary),
            shortterm: load_f32(&self.shortterm),
            integrated: load_f32(&self.integrated),
            tp_l: load_f32(&self.tp_l),
            tp_r: load_f32(&self.tp_r),
            lra: load_f32(&self.lra),
            rms: load_f32(&self.rms),
            noise_floor: load_f32(&self.noise_floor),
            sample_peak: load_f32(&self.sample_peak),
            dc_offset: load_f32(&self.dc_offset),
            correlation: load_f32(&self.correlation),
            clips: self.clips.load(Ordering::Relaxed),
        }
    }
}

const LUFS_MODE: Mode = Mode::I
    .union(Mode::M)
    .union(Mode::S)
    .union(Mode::LRA)
    .union(Mode::TRUE_PEAK);

impl LufsMeterEffect {
    pub fn new(_d: LufsMeterData, node_id: String, sample_rate: u32) -> (Self, LufsHandle) {
        let ebu = EbuR128::new(2, sample_rate, LUFS_MODE).ok();
        let ebu_mono = EbuR128::new(1, sample_rate, LUFS_MODE).ok();
        let handle = LufsHandle {
            node_id,
            momentary: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            shortterm: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            integrated: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            tp_l: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            tp_r: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            lra: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            rms: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            noise_floor: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            sample_peak: Arc::new(AtomicU32::new(LUFS_SILENT.to_bits())),
            dc_offset: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            correlation: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            clips: Arc::new(AtomicU32::new(0)),
        };
        (
            Self {
                ebu,
                ebu_mono,
                scratch: vec![0.0; SCRATCH_FRAMES * 2],
                handle: handle.clone(),
                frames_since_global: 0,
                sample_rate,
                sum_sq: 0.0,
                total_frames: 0,
                win_sum_sq: 0.0,
                win_frames: 0,
                window_frames: (sample_rate as usize / 1000) * NOISE_WINDOW_MS,
                noise_floor: f32::INFINITY,
                sum_dc: 0.0,
                sample_peak: 0.0,
                clips: 0,
                win_lr: 0.0,
                win_ll: 0.0,
                win_rr: 0.0,
            },
            handle,
        )
    }

    pub fn from_handle(handle: LufsHandle, sample_rate: u32) -> Self {
        let ebu = EbuR128::new(2, sample_rate, LUFS_MODE).ok();
        let ebu_mono = EbuR128::new(1, sample_rate, LUFS_MODE).ok();
        Self {
            ebu,
            ebu_mono,
            scratch: vec![0.0; SCRATCH_FRAMES * 2],
            handle,
            frames_since_global: 0,
            sample_rate,
            sum_sq: 0.0,
            total_frames: 0,
            win_sum_sq: 0.0,
            win_frames: 0,
            window_frames: (sample_rate as usize / 1000) * NOISE_WINDOW_MS,
            noise_floor: f32::INFINITY,
            sum_dc: 0.0,
            sample_peak: 0.0,
            clips: 0,
            win_lr: 0.0,
            win_ll: 0.0,
            win_rr: 0.0,
        }
    }
}

impl Effect for LufsMeterEffect {
    fn process(&mut self, samples: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        let channels = (samples.len() / frames).max(1);
        self.accumulate_rms(&samples[..frames * channels], frames, channels);

        // The node caps its inputs at two, so a wider block only reaches here if
        // an upstream width forced it; judge its first two channels.
        let (ebu, fed) = if channels == 1 {
            (self.ebu_mono.as_mut(), &samples[..frames])
        } else if channels == 2 {
            (self.ebu.as_mut(), &samples[..frames * 2])
        } else {
            let n = frames.min(SCRATCH_FRAMES);
            for f in 0..n {
                self.scratch[f * 2] = samples[f * channels];
                self.scratch[f * 2 + 1] = samples[f * channels + 1];
            }
            (self.ebu.as_mut(), &self.scratch[..n * 2])
        };
        let Some(ebu) = ebu else { return };
        // ebur128's internal buffers are pre-allocated by `new` — `add_frames`
        // stays alloc-free on the RT path.
        let _ = ebu.add_frames_f32(fed);
        let m = ebu.loudness_momentary().unwrap_or(f64::NEG_INFINITY);
        let s = ebu.loudness_shortterm().unwrap_or(f64::NEG_INFINITY);
        store_f32(&self.handle.momentary, sanitize_lufs(m));
        store_f32(&self.handle.shortterm, sanitize_lufs(s));

        let tp_l = ebu.true_peak(0).unwrap_or(0.0);
        let tp_r = ebu.true_peak(1).unwrap_or(tp_l);
        store_f32(&self.handle.tp_l, amp_to_db(tp_l));
        store_f32(&self.handle.tp_r, amp_to_db(tp_r));

        self.frames_since_global += frames;
        if self.frames_since_global >= self.sample_rate as usize {
            let i = ebu.loudness_global().unwrap_or(f64::NEG_INFINITY);
            store_f32(&self.handle.integrated, sanitize_lufs(i));
            let lra = ebu.loudness_range().unwrap_or(0.0);
            store_f32(&self.handle.lra, lra as f32);
            self.frames_since_global = 0;
        }
    }
}

impl LufsMeterEffect {
    /// Interleaved stereo in, one RMS across both channels: ACX judges the
    /// delivered file, which is a single mono or stereo program.
    fn accumulate_rms(&mut self, interleaved: &[f32], frames: usize, channels: usize) {
        let mut block_sq = 0.0f64;
        let mut block_dc = 0.0f64;
        for v in interleaved {
            let x = *v;
            block_sq += (x as f64) * (x as f64);
            block_dc += x as f64;
            let a = x.abs();
            if a > self.sample_peak {
                self.sample_peak = a;
            }
            if a >= CLIP_LEVEL {
                self.clips = self.clips.saturating_add(1);
            }
        }
        // Correlation needs a pair; a mono node has no phase relationship to show.
        if channels >= 2 {
            for f in 0..frames {
                let l = interleaved[f * channels] as f64;
                let r = interleaved[f * channels + 1] as f64;
                self.win_lr += l * r;
                self.win_ll += l * l;
                self.win_rr += r * r;
            }
        }
        let samples = (frames * channels) as u64;

        self.sum_dc += block_dc;
        store_f32(&self.handle.sample_peak, amp_to_db(self.sample_peak as f64));
        self.handle.clips.store(self.clips, Ordering::Relaxed);

        self.sum_sq += block_sq;
        self.total_frames += samples;
        if self.total_frames > 0 {
            let rms = (self.sum_sq / self.total_frames as f64).sqrt();
            store_f32(&self.handle.rms, amp_to_db(rms));
        }

        self.win_sum_sq += block_sq;
        self.win_frames += frames;
        if self.win_frames >= self.window_frames {
            let win_rms = (self.win_sum_sq / (self.win_frames * channels) as f64).sqrt();
            let db = amp_to_db(win_rms);
            if db > NOISE_IGNORE_BELOW && db < self.noise_floor {
                self.noise_floor = db;
                store_f32(&self.handle.noise_floor, db);
            }
            let denom = (self.win_ll * self.win_rr).sqrt();
            // Silence has no phase relationship; reporting 0 there would look
            // like a cancellation warning.
            let corr = if denom > 1e-12 {
                (self.win_lr / denom).clamp(-1.0, 1.0) as f32
            } else {
                1.0
            };
            store_f32(&self.handle.correlation, corr);

            self.win_sum_sq = 0.0;
            self.win_frames = 0;
            self.win_lr = 0.0;
            self.win_ll = 0.0;
            self.win_rr = 0.0;
        }

        if self.total_frames > 0 {
            store_f32(
                &self.handle.dc_offset,
                (self.sum_dc / self.total_frames as f64) as f32,
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::effects::Effect;

    fn meter() -> LufsMeterEffect {
        LufsMeterEffect::new(LufsMeterData {}, "test".into(), 48_000).0
    }

    fn run(channels: usize, frames: usize, value: f32) -> LufsMeterEffect {
        let mut m = meter();
        let mut buf = vec![value; frames * channels];
        m.process(&mut buf, frames);
        m
    }

    #[test]
    fn any_channel_width_is_accepted() {
        // A single wired channel used to slice the block as stereo and panic.
        for channels in [1usize, 2, 3, 6] {
            run(channels, 512, 0.25);
        }
    }

    #[test]
    fn rms_of_full_scale_dc_is_zero_dbfs() {
        let m = run(2, 4096, 1.0);
        let rms = load_f32(&m.handle.rms);
        assert!((rms - 0.0).abs() < 0.1, "expected ~0 dBFS, got {rms}");
    }

    #[test]
    fn clipping_is_counted_per_sample() {
        let m = run(2, 100, 1.0);
        assert_eq!(m.handle.clips.load(Ordering::Relaxed), 200);
    }

    #[test]
    fn identical_channels_correlate_positively() {
        let mut m = meter();
        let mut buf: Vec<f32> = (0..48_000 * 2)
            .map(|i| ((i / 2) as f32 * 0.01).sin() * 0.5)
            .collect();
        m.process(&mut buf, 48_000);
        let corr = load_f32(&m.handle.correlation);
        assert!(corr > 0.99, "expected ~1.0, got {corr}");
    }
}
