//! Real-time DSP effects. All effects operate on interleaved stereo f32 frames.
//!
//! Parameters live in `Arc<Atomic*>` cells shared with the UI side of the
//! engine. The audio callback reads them lock-free on every block, so slider
//! moves and mute toggles take effect within a couple of milliseconds without
//! restarting the pipeline.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::Value;

use crate::audio::graph::EffectSpec;

pub mod biquad;
pub mod channel_balance;
pub mod compressor;
pub mod delay;
pub mod eq;
pub mod gain;
pub mod level_meter;
pub mod limiter;
pub mod lufs_meter;
pub mod mute;
pub mod noise_gate;
pub mod noise_suppressor;
pub mod waveform;
pub mod reverb;
pub mod saturator;
mod util;

use util::{db_to_linear, num, store_f32};

use channel_balance::ChannelBalanceEffect;
use compressor::CompressorEffect;
use delay::DelayEffect;
use eq::EqEffect;
use gain::GainEffect;
use limiter::LimiterEffect;
use mute::MuteEffect;
use noise_gate::NoiseGateEffect;
use noise_suppressor::NoiseSuppressorEffect;
use reverb::ReverbEffect;
use saturator::SaturatorEffect;
pub use level_meter::{update_meter, LevelMeterEffect, MeterHandle};
pub use lufs_meter::{LufsHandle, LufsMeterEffect};
pub use waveform::{WaveformEffect, WaveformHandle};

/// Shared atom for a dynamic-gain effect (compressor / noise gate / limiter).
/// The audio thread writes the block's minimum gain each block; the meter tick
/// thread reads it and emits `audio://gr` to the frontend.
#[derive(Clone)]
pub struct GrHandle {
    pub node_id: String,
    pub gr_lin: Arc<AtomicU32>,
}

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
    Waveform(WaveformEffect),
    Limiter(LimiterEffect),
    Compressor(CompressorEffect),
    NoiseGate(NoiseGateEffect),
    Delay(DelayEffect),
    Reverb(ReverbEffect),
    NoiseSuppressor(NoiseSuppressorEffect),
}

impl RuntimeEffect {
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
            RuntimeEffect::Waveform(e) => e.latency_frames(),
            RuntimeEffect::Limiter(e) => e.latency_frames(),
            RuntimeEffect::Compressor(e) => e.latency_frames(),
            RuntimeEffect::NoiseGate(e) => e.latency_frames(),
            RuntimeEffect::Delay(e) => e.latency_frames(),
            RuntimeEffect::Reverb(e) => e.latency_frames(),
            RuntimeEffect::NoiseSuppressor(e) => e.latency_frames(),
        }
    }

    #[inline]
    pub fn process_with_sidechain(
        &mut self,
        main: &mut [f32],
        sidechain: Option<&[f32]>,
        frames: usize,
    ) {
        match self {
            RuntimeEffect::Compressor(e) => e.process_with_sidechain(main, sidechain, frames),
            RuntimeEffect::NoiseGate(e) => e.process_with_sidechain(main, sidechain, frames),
            RuntimeEffect::Gain(e) => e.process(main, frames),
            RuntimeEffect::Mute(e) => e.process(main, frames),
            RuntimeEffect::ChannelBalance(e) => e.process(main, frames),
            RuntimeEffect::Saturator(e) => e.process(main, frames),
            RuntimeEffect::Eq(e) => e.process(main, frames),
            RuntimeEffect::LevelMeter(e) => e.process(main, frames),
            RuntimeEffect::LufsMeter(e) => e.process(main, frames),
            RuntimeEffect::Waveform(e) => e.process(main, frames),
            RuntimeEffect::Limiter(e) => e.process(main, frames),
            RuntimeEffect::Delay(e) => e.process(main, frames),
            RuntimeEffect::Reverb(e) => e.process(main, frames),
            RuntimeEffect::NoiseSuppressor(e) => e.process(main, frames),
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
    Compressor {
        threshold_db: Arc<AtomicU32>,
        ratio: Arc<AtomicU32>,
        attack_ms: Arc<AtomicU32>,
        release_ms: Arc<AtomicU32>,
        knee_db: Arc<AtomicU32>,
        makeup_db: Arc<AtomicU32>,
    },
    NoiseGate {
        threshold_db: Arc<AtomicU32>,
        range_db: Arc<AtomicU32>,
        attack_ms: Arc<AtomicU32>,
        hold_ms: Arc<AtomicU32>,
        release_ms: Arc<AtomicU32>,
    },
    Delay {
        time_ms: Arc<AtomicU32>,
        feedback: Arc<AtomicU32>,
        mix: Arc<AtomicU32>,
    },
    Reverb {
        room_size: Arc<AtomicU32>,
        damping: Arc<AtomicU32>,
        width: Arc<AtomicU32>,
        mix: Arc<AtomicU32>,
    },
    NoiseSuppressor {
        attenuation_limit_db: Arc<AtomicU32>,
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
            EffectControl::Compressor {
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                knee_db,
                makeup_db,
            } => {
                if let Some(v) = num(data, "thresholdDb") { store_f32(threshold_db, v); }
                if let Some(v) = num(data, "ratio") { store_f32(ratio, v.max(1.0)); }
                if let Some(v) = num(data, "attackMs") { store_f32(attack_ms, v.max(0.01)); }
                if let Some(v) = num(data, "releaseMs") { store_f32(release_ms, v.max(0.1)); }
                if let Some(v) = num(data, "kneeDb") { store_f32(knee_db, v.max(0.0)); }
                if let Some(v) = num(data, "makeupDb") { store_f32(makeup_db, v); }
            }
            EffectControl::NoiseGate {
                threshold_db,
                range_db,
                attack_ms,
                hold_ms,
                release_ms,
            } => {
                if let Some(v) = num(data, "thresholdDb") { store_f32(threshold_db, v); }
                if let Some(v) = num(data, "rangeDb") { store_f32(range_db, v.min(0.0)); }
                if let Some(v) = num(data, "attackMs") { store_f32(attack_ms, v.max(0.01)); }
                if let Some(v) = num(data, "holdMs") { store_f32(hold_ms, v.max(0.0)); }
                if let Some(v) = num(data, "releaseMs") { store_f32(release_ms, v.max(0.1)); }
            }
            EffectControl::Delay { time_ms, feedback, mix } => {
                if let Some(v) = num(data, "timeMs") { store_f32(time_ms, v.max(1.0)); }
                if let Some(v) = num(data, "feedback") { store_f32(feedback, v.clamp(0.0, 0.95)); }
                if let Some(v) = num(data, "mix") { store_f32(mix, v.clamp(0.0, 1.0)); }
            }
            EffectControl::Reverb { room_size, damping, width, mix } => {
                if let Some(v) = num(data, "roomSize") { store_f32(room_size, v.clamp(0.0, 1.0)); }
                if let Some(v) = num(data, "damping") { store_f32(damping, v.clamp(0.0, 1.0)); }
                if let Some(v) = num(data, "width") { store_f32(width, v.clamp(0.0, 1.0)); }
                if let Some(v) = num(data, "mix") { store_f32(mix, v.clamp(0.0, 1.0)); }
            }
            EffectControl::NoiseSuppressor { attenuation_limit_db } => {
                if let Some(v) = num(data, "attenuationLimitDb") {
                    store_f32(attenuation_limit_db, v.max(0.0));
                }
            }
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
    /// Some only on the first instantiation per node id, for GR-capable effects.
    pub gr: Option<GrHandle>,
    /// Some only on the first instantiation per node id, for oscilloscope nodes.
    pub scope: Option<WaveformHandle>,
    pub bypass: Arc<AtomicBool>,
    pub bypass_is_new: bool,
}

/// Shared atomics keyed by node id so a fan-out effect (one node feeding
/// multiple outputs) keeps live params in sync across instances.
#[derive(Default)]
pub struct EffectRegistry {
    controls: std::collections::HashMap<String, EffectControl>,
    bypasses: std::collections::HashMap<String, Arc<AtomicBool>>,
    meters: std::collections::HashMap<String, MeterHandle>,
    lufs: std::collections::HashMap<String, LufsHandle>,
    gr_atomics: std::collections::HashMap<String, Arc<AtomicU32>>,
    scopes: std::collections::HashMap<String, WaveformHandle>,
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
    let (bypass, bypass_is_new) = match registry.bypasses.get(node_id) {
        Some(b) => (b.clone(), false),
        None => {
            let b = Arc::new(AtomicBool::new(spec.bypassed()));
            registry.bypasses.insert(node_id.to_string(), b.clone());
            (b, true)
        }
    };
    let mk = |effect: RuntimeEffect,
              control: Option<EffectControl>,
              meter: Option<MeterHandle>,
              lufs: Option<LufsHandle>,
              gr: Option<GrHandle>,
              scope: Option<WaveformHandle>| EffectBuild {
        effect,
        control,
        meter,
        lufs,
        gr,
        scope,
        bypass: bypass.clone(),
        bypass_is_new,
    };
    match *spec {
        EffectSpec::Gain(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Gain { linear }) => mk(
                RuntimeEffect::Gain(GainEffect::from_state(linear.clone())),
                None, None, None, None, None,
            ),
            _ => {
                let (e, c) = GainEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Gain(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Mute(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Mute { muted }) => mk(
                RuntimeEffect::Mute(MuteEffect::from_state(muted.clone())),
                None, None, None, None, None,
            ),
            _ => {
                let (e, c) = MuteEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Mute(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::ChannelBalance(d) => match registry.controls.get(node_id) {
            Some(EffectControl::ChannelBalance { left, right }) => mk(
                RuntimeEffect::ChannelBalance(ChannelBalanceEffect::from_state(
                    left.clone(),
                    right.clone(),
                )),
                None, None, None, None, None,
            ),
            _ => {
                let (e, c) = ChannelBalanceEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::ChannelBalance(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Saturator(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Saturator { ceiling, drive }) => mk(
                RuntimeEffect::Saturator(SaturatorEffect::from_state(
                    ceiling.clone(),
                    drive.clone(),
                )),
                None, None, None, None, None,
            ),
            _ => {
                let (e, c) = SaturatorEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Saturator(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Eq(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Eq { gains }) => mk(
                RuntimeEffect::Eq(EqEffect::from_state(gains.clone(), sample_rate)),
                None, None, None, None, None,
            ),
            _ => {
                let (e, c) = EqEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Eq(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::LevelMeter(d) => match registry.meters.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::LevelMeter(LevelMeterEffect::from_handle(handle.clone())),
                None, None, None, None, None,
            ),
            None => {
                let (e, handle) = LevelMeterEffect::new(d, node_id.to_string());
                registry.meters.insert(node_id.to_string(), handle.clone());
                mk(RuntimeEffect::LevelMeter(e), None, Some(handle), None, None, None)
            }
        },
        EffectSpec::LufsMeter(d) => match registry.lufs.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::LufsMeter(LufsMeterEffect::from_handle(handle.clone(), sample_rate)),
                None, None, None, None, None,
            ),
            None => {
                let (e, handle) = LufsMeterEffect::new(d, node_id.to_string(), sample_rate);
                registry.lufs.insert(node_id.to_string(), handle.clone());
                mk(RuntimeEffect::LufsMeter(e), None, None, Some(handle), None, None)
            }
        },
        EffectSpec::Waveform(d) => match registry.scopes.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::Waveform(WaveformEffect::from_handle(handle.clone())),
                None, None, None, None, None,
            ),
            None => {
                let (e, handle) = WaveformEffect::new(d, node_id.to_string());
                registry.scopes.insert(node_id.to_string(), handle.clone());
                mk(RuntimeEffect::Waveform(e), None, None, None, None, Some(handle))
            }
        },
        EffectSpec::Limiter(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Limiter { ceiling, release_ms }) => {
                let lookahead_frames =
                    ((d.lookahead_ms.max(0.1) * sample_rate as f32 / 1000.0) as usize).max(1);
                let gr_arc = registry.gr_atomics.get(node_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                mk(
                    RuntimeEffect::Limiter(LimiterEffect::from_state(
                        ceiling.clone(),
                        release_ms.clone(),
                        lookahead_frames,
                        sample_rate,
                        gr_arc,
                    )),
                    None, None, None, None, None,
                )
            }
            _ => {
                let (e, c, gr_arc) = LimiterEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                registry.gr_atomics.insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle { node_id: node_id.to_string(), gr_lin: gr_arc };
                mk(RuntimeEffect::Limiter(e), Some(c), None, None, Some(gr), None)
            }
        },
        EffectSpec::Compressor(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Compressor {
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                knee_db,
                makeup_db,
            }) => {
                let gr_arc = registry.gr_atomics.get(node_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                mk(
                    RuntimeEffect::Compressor(CompressorEffect::from_state(
                        threshold_db.clone(),
                        ratio.clone(),
                        attack_ms.clone(),
                        release_ms.clone(),
                        knee_db.clone(),
                        makeup_db.clone(),
                        sample_rate,
                        gr_arc,
                    )),
                    None, None, None, None, None,
                )
            }
            _ => {
                let (e, c, gr_arc) = CompressorEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                registry.gr_atomics.insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle { node_id: node_id.to_string(), gr_lin: gr_arc };
                mk(RuntimeEffect::Compressor(e), Some(c), None, None, Some(gr), None)
            }
        },
        EffectSpec::NoiseGate(d) => match registry.controls.get(node_id) {
            Some(EffectControl::NoiseGate {
                threshold_db,
                range_db,
                attack_ms,
                hold_ms,
                release_ms,
            }) => {
                let gr_arc = registry.gr_atomics.get(node_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                mk(
                    RuntimeEffect::NoiseGate(NoiseGateEffect::from_state(
                        threshold_db.clone(),
                        range_db.clone(),
                        attack_ms.clone(),
                        hold_ms.clone(),
                        release_ms.clone(),
                        sample_rate,
                        gr_arc,
                    )),
                    None, None, None, None, None,
                )
            }
            _ => {
                let (e, c, gr_arc) = NoiseGateEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                registry.gr_atomics.insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle { node_id: node_id.to_string(), gr_lin: gr_arc };
                mk(RuntimeEffect::NoiseGate(e), Some(c), None, None, Some(gr), None)
            }
        },
        EffectSpec::Delay(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Delay { time_ms, feedback, mix }) => mk(
                RuntimeEffect::Delay(DelayEffect::from_state(
                    time_ms.clone(),
                    feedback.clone(),
                    mix.clone(),
                    sample_rate,
                )),
                None, None, None, None, None,
            ),
            _ => {
                let (e, c) = DelayEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Delay(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Reverb(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Reverb { room_size, damping, width, mix }) => mk(
                RuntimeEffect::Reverb(ReverbEffect::from_state(
                    room_size.clone(),
                    damping.clone(),
                    width.clone(),
                    mix.clone(),
                    sample_rate,
                )),
                None, None, None, None, None,
            ),
            _ => {
                let (e, c) = ReverbEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Reverb(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::NoiseSuppressor(d) => match registry.controls.get(node_id) {
            Some(EffectControl::NoiseSuppressor { attenuation_limit_db }) => mk(
                RuntimeEffect::NoiseSuppressor(NoiseSuppressorEffect::from_state(
                    attenuation_limit_db.clone(),
                    sample_rate,
                )),
                None, None, None, None, None,
            ),
            _ => {
                let (e, c) = NoiseSuppressorEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::NoiseSuppressor(e), Some(c), None, None, None, None)
            }
        },
    }
}
