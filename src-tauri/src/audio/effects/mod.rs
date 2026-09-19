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
use crate::audio::plugins::host_api::HostedEffect;

/// Fixed DSP block size; hosted-plugin scratch buffers are sized to it. Must
/// stay >= the pipeline's `DSP_BLOCK_FRAMES`, or a block would overrun them.
const PLUGIN_MAX_BLOCK: usize = 1024;

pub mod biquad;
pub mod channel_balance;
pub mod compressor;
pub mod de_esser;
pub mod declick;
pub mod delay;
pub mod eq;
pub mod gain;
pub mod level_meter;
pub mod limiter;
pub mod lufs_meter;
pub mod mute;
pub mod noise_gate;
pub mod noise_suppressor;
pub(crate) mod offload;
pub mod reverb;
pub mod saturator;
mod util;
pub mod waveform;

use util::{db_to_linear, num, store_f32};

use channel_balance::ChannelBalanceEffect;
use compressor::CompressorEffect;
use de_esser::DeEsserEffect;
use declick::DeclickEffect;
use delay::DelayEffect;
use eq::EqEffect;
use gain::GainEffect;
pub use level_meter::{update_meter, LevelMeterEffect, MeterHandle};
use limiter::LimiterEffect;
pub use lufs_meter::{LufsHandle, LufsMeterEffect};
use mute::MuteEffect;
use noise_gate::NoiseGateEffect;
use noise_suppressor::{NoiseSuppressorControls, NoiseSuppressorEffect};
use reverb::ReverbEffect;
use saturator::SaturatorEffect;
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
    Declick(DeclickEffect),
    DeEsser(DeEsserEffect),
    HostedPlugin(HostedEffect),
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
            RuntimeEffect::Declick(e) => e.latency_frames(),
            RuntimeEffect::DeEsser(e) => e.latency_frames(),
            RuntimeEffect::HostedPlugin(e) => e.latency_frames(),
        }
    }

    #[inline]
    pub fn process_with_sidechain(
        &mut self,
        main: &mut [f32],
        sidechain: Option<&[f32]>,
        frames: usize,
    ) {
        let active = &mut main[..frames * 2];
        for sample in active.iter_mut() {
            if !sample.is_finite() {
                *sample = 0.0;
            }
        }
        match self {
            RuntimeEffect::Compressor(e) => e.process_with_sidechain(active, sidechain, frames),
            RuntimeEffect::NoiseGate(e) => e.process_with_sidechain(active, sidechain, frames),
            RuntimeEffect::Gain(e) => e.process(active, frames),
            RuntimeEffect::Mute(e) => e.process(active, frames),
            RuntimeEffect::ChannelBalance(e) => e.process(active, frames),
            RuntimeEffect::Saturator(e) => e.process(active, frames),
            RuntimeEffect::Eq(e) => e.process(active, frames),
            RuntimeEffect::LevelMeter(e) => e.process(active, frames),
            RuntimeEffect::LufsMeter(e) => e.process(active, frames),
            RuntimeEffect::Waveform(e) => e.process(active, frames),
            RuntimeEffect::Limiter(e) => e.process(active, frames),
            RuntimeEffect::Delay(e) => e.process(active, frames),
            RuntimeEffect::Reverb(e) => e.process(active, frames),
            RuntimeEffect::NoiseSuppressor(e) => e.process(active, frames),
            RuntimeEffect::Declick(e) => e.process(active, frames),
            RuntimeEffect::DeEsser(e) => e.process(active, frames),
            RuntimeEffect::HostedPlugin(e) => e.process(active, frames),
        }
        for sample in active {
            if !sample.is_finite() {
                *sample = 0.0;
            }
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
        controls: NoiseSuppressorControls,
    },
    Declick {
        sensitivity: Arc<AtomicU32>,
        max_width_ms: Arc<AtomicU32>,
    },
    DeEsser {
        frequency: Arc<AtomicU32>,
        threshold_db: Arc<AtomicU32>,
        ratio: Arc<AtomicU32>,
    },
    Plugin {
        // Shared with the RT `PluginNode`; UI param writes flow through it.
        events: Arc<crate::audio::plugins::ParamRing>,
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
            EffectControl::Limiter {
                ceiling,
                release_ms,
            } => {
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
                if let Some(v) = num(data, "thresholdDb") {
                    store_f32(threshold_db, v);
                }
                if let Some(v) = num(data, "ratio") {
                    store_f32(ratio, v.max(1.0));
                }
                if let Some(v) = num(data, "attackMs") {
                    store_f32(attack_ms, v.max(0.01));
                }
                if let Some(v) = num(data, "releaseMs") {
                    store_f32(release_ms, v.max(0.1));
                }
                if let Some(v) = num(data, "kneeDb") {
                    store_f32(knee_db, v.max(0.0));
                }
                if let Some(v) = num(data, "makeupDb") {
                    store_f32(makeup_db, v);
                }
            }
            EffectControl::NoiseGate {
                threshold_db,
                range_db,
                attack_ms,
                hold_ms,
                release_ms,
            } => {
                if let Some(v) = num(data, "thresholdDb") {
                    store_f32(threshold_db, v);
                }
                if let Some(v) = num(data, "rangeDb") {
                    store_f32(range_db, v.min(0.0));
                }
                if let Some(v) = num(data, "attackMs") {
                    store_f32(attack_ms, v.max(0.01));
                }
                if let Some(v) = num(data, "holdMs") {
                    store_f32(hold_ms, v.max(0.0));
                }
                if let Some(v) = num(data, "releaseMs") {
                    store_f32(release_ms, v.max(0.1));
                }
            }
            EffectControl::Delay {
                time_ms,
                feedback,
                mix,
            } => {
                if let Some(v) = num(data, "timeMs") {
                    store_f32(time_ms, v.max(1.0));
                }
                if let Some(v) = num(data, "feedback") {
                    store_f32(feedback, v.clamp(0.0, 0.95));
                }
                if let Some(v) = num(data, "mix") {
                    store_f32(mix, v.clamp(0.0, 1.0));
                }
            }
            EffectControl::Reverb {
                room_size,
                damping,
                width,
                mix,
            } => {
                if let Some(v) = num(data, "roomSize") {
                    store_f32(room_size, v.clamp(0.0, 1.0));
                }
                if let Some(v) = num(data, "damping") {
                    store_f32(damping, v.clamp(0.0, 1.0));
                }
                if let Some(v) = num(data, "width") {
                    store_f32(width, v.clamp(0.0, 1.0));
                }
                if let Some(v) = num(data, "mix") {
                    store_f32(mix, v.clamp(0.0, 1.0));
                }
            }
            EffectControl::NoiseSuppressor { controls } => {
                if let Some(v) = num(data, "attenuationLimitDb") {
                    store_f32(&controls.atten_lim_db, v.max(0.0));
                }
                if let Some(v) = num(data, "postFilterBeta") {
                    store_f32(&controls.pf_beta, v.max(0.0));
                }
                if let Some(v) = num(data, "minThreshDb") {
                    store_f32(&controls.min_thresh_db, v);
                }
                if let Some(v) = num(data, "maxErbThreshDb") {
                    store_f32(&controls.max_erb_thresh_db, v);
                }
                if let Some(v) = num(data, "maxDfThreshDb") {
                    store_f32(&controls.max_df_thresh_db, v);
                }
            }
            EffectControl::Declick {
                sensitivity,
                max_width_ms,
            } => {
                if let Some(v) = num(data, "sensitivity") {
                    store_f32(sensitivity, v.clamp(0.0, 1.0));
                }
                if let Some(v) = num(data, "maxWidthMs") {
                    store_f32(max_width_ms, v.clamp(0.3, 5.0));
                }
            }
            EffectControl::DeEsser {
                frequency,
                threshold_db,
                ratio,
            } => {
                if let Some(v) = num(data, "frequency") {
                    store_f32(frequency, v.clamp(2000.0, 16000.0));
                }
                if let Some(v) = num(data, "thresholdDb") {
                    store_f32(threshold_db, v.clamp(-80.0, 0.0));
                }
                if let Some(v) = num(data, "ratio") {
                    store_f32(ratio, v.clamp(1.0, 12.0));
                }
            }
            EffectControl::Plugin { events } => {
                // `{ pluginParams: { "<paramId>": value } }` from the node UI.
                if let Some(map) = data.get("pluginParams").and_then(Value::as_object) {
                    for (id, v) in map {
                        if let (Ok(id), Some(value)) = (id.parse::<u32>(), v.as_f64()) {
                            events.push(id, value);
                        }
                    }
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
    /// The effect took the node's whole width, so the pipeline must hand it
    /// every channel at once instead of splitting into stereo pairs.
    pub full_width: bool,
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
    // Per-plugin UI->RT parameter queue, reused across rebuilds so the control
    // handed to the frontend keeps reaching the current `PluginNode`.
    plugin_param_rings: std::collections::HashMap<String, Arc<crate::audio::plugins::ParamRing>>,
    // Plugin node ids that already claimed the editor-target (primary) instance
    // in the current reconcile. A node feeding several real outputs is built
    // once per output; only the first claim owns the editor, the rest are
    // metering/duplicate instances parked in the graveyard.
    plugin_primary_claimed: std::collections::HashSet<String>,
}

impl EffectRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears per-reconcile scratch. Call once before rebuilding the graphs so
    /// the primary-instance claim is decided fresh each pass.
    pub fn begin_reconcile(&mut self) {
        self.plugin_primary_claimed.clear();
    }
}

impl Drop for EffectRegistry {
    /// A host keeps its own instance per plugin node so the UI thread can reach
    /// it. Nothing else marks the end of a pipeline's life, so the registry
    /// releases those holds as it goes; the instances themselves live on until
    /// their RT nodes are dropped too.
    fn drop(&mut self) {
        for node_id in self.plugin_param_rings.keys() {
            crate::audio::plugins::registry::forget(node_id);
        }
    }
}

pub fn instantiate_effect(
    spec: &EffectSpec,
    node_id: &str,
    sample_rate: u32,
    // False for file-recording outputs: an offline render outruns real time,
    // so an expensive effect must process in place instead of on a worker.
    realtime: bool,
    // False when building the monitor graph: a plugin instantiated there is a
    // metering-only duplicate, not the one its editor window attaches to.
    primary: bool,
    // Channels this node carries. An effect that can take them all says so
    // through `EffectBuild::full_width`; the rest are driven one pair at a time.
    channels: usize,
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
        full_width: false,
    };
    match *spec {
        EffectSpec::Gain(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Gain { linear }) => mk(
                RuntimeEffect::Gain(GainEffect::from_state(linear.clone())),
                None,
                None,
                None,
                None,
                None,
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
                None,
                None,
                None,
                None,
                None,
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
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = ChannelBalanceEffect::new(d);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(
                    RuntimeEffect::ChannelBalance(e),
                    Some(c),
                    None,
                    None,
                    None,
                    None,
                )
            }
        },
        EffectSpec::Saturator(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Saturator { ceiling, drive }) => mk(
                RuntimeEffect::Saturator(SaturatorEffect::from_state(
                    ceiling.clone(),
                    drive.clone(),
                )),
                None,
                None,
                None,
                None,
                None,
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
                None,
                None,
                None,
                None,
                None,
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
                None,
                None,
                None,
                None,
                None,
            ),
            None => {
                let (e, handle) = LevelMeterEffect::new(d, node_id.to_string());
                registry.meters.insert(node_id.to_string(), handle.clone());
                mk(
                    RuntimeEffect::LevelMeter(e),
                    None,
                    Some(handle),
                    None,
                    None,
                    None,
                )
            }
        },
        EffectSpec::LufsMeter(d) => match registry.lufs.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::LufsMeter(LufsMeterEffect::from_handle(handle.clone(), sample_rate)),
                None,
                None,
                None,
                None,
                None,
            ),
            None => {
                let (e, handle) = LufsMeterEffect::new(d, node_id.to_string(), sample_rate);
                registry.lufs.insert(node_id.to_string(), handle.clone());
                mk(
                    RuntimeEffect::LufsMeter(e),
                    None,
                    None,
                    Some(handle),
                    None,
                    None,
                )
            }
        },
        EffectSpec::Waveform(d) => match registry.scopes.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::Waveform(WaveformEffect::from_handle(handle.clone())),
                None,
                None,
                None,
                None,
                None,
            ),
            None => {
                let (e, handle) = WaveformEffect::new(d, node_id.to_string(), sample_rate);
                registry.scopes.insert(node_id.to_string(), handle.clone());
                mk(
                    RuntimeEffect::Waveform(e),
                    None,
                    None,
                    None,
                    None,
                    Some(handle),
                )
            }
        },
        // Spectrum reuses the scope's time-domain capture and SCOPE_EVENT
        // transport; the UI runs the FFT. No distinct runtime effect is needed.
        EffectSpec::Spectrum(_) => match registry.scopes.get(node_id) {
            Some(handle) => mk(
                RuntimeEffect::Waveform(WaveformEffect::from_handle(handle.clone())),
                None,
                None,
                None,
                None,
                None,
            ),
            None => {
                let (e, handle) = WaveformEffect::new_for(node_id.to_string(), sample_rate);
                registry.scopes.insert(node_id.to_string(), handle.clone());
                mk(
                    RuntimeEffect::Waveform(e),
                    None,
                    None,
                    None,
                    None,
                    Some(handle),
                )
            }
        },
        EffectSpec::Limiter(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Limiter {
                ceiling,
                release_ms,
            }) => {
                let lookahead_frames =
                    ((d.lookahead_ms.max(0.1) * sample_rate as f32 / 1000.0) as usize).max(1);
                let gr_arc = registry
                    .gr_atomics
                    .get(node_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                // Republished on every rebuild: without this the meter thread
                // loses the handle after the first reconcile and the readout
                // freezes at its initial value.
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc.clone(),
                };
                mk(
                    RuntimeEffect::Limiter(LimiterEffect::from_state(
                        ceiling.clone(),
                        release_ms.clone(),
                        lookahead_frames,
                        sample_rate,
                        gr_arc,
                    )),
                    None,
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
            _ => {
                let (e, c, gr_arc) = LimiterEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc,
                };
                mk(
                    RuntimeEffect::Limiter(e),
                    Some(c),
                    None,
                    None,
                    Some(gr),
                    None,
                )
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
                let gr_arc = registry
                    .gr_atomics
                    .get(node_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                // Republished on every rebuild: without this the meter thread
                // loses the handle after the first reconcile and the readout
                // freezes at its initial value.
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc.clone(),
                };
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
                    None,
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
            _ => {
                let (e, c, gr_arc) = CompressorEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc,
                };
                mk(
                    RuntimeEffect::Compressor(e),
                    Some(c),
                    None,
                    None,
                    Some(gr),
                    None,
                )
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
                let gr_arc = registry
                    .gr_atomics
                    .get(node_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(AtomicU32::new(1.0f32.to_bits())));
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                // Republished on every rebuild: without this the meter thread
                // loses the handle after the first reconcile and the readout
                // freezes at its initial value.
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc.clone(),
                };
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
                    None,
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
            _ => {
                let (e, c, gr_arc) = NoiseGateEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                registry
                    .gr_atomics
                    .insert(node_id.to_string(), gr_arc.clone());
                let gr = GrHandle {
                    node_id: node_id.to_string(),
                    gr_lin: gr_arc,
                };
                mk(
                    RuntimeEffect::NoiseGate(e),
                    Some(c),
                    None,
                    None,
                    Some(gr),
                    None,
                )
            }
        },
        EffectSpec::Delay(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Delay {
                time_ms,
                feedback,
                mix,
            }) => mk(
                RuntimeEffect::Delay(DelayEffect::from_state(
                    time_ms.clone(),
                    feedback.clone(),
                    mix.clone(),
                    sample_rate,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = DelayEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Delay(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Reverb(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Reverb {
                room_size,
                damping,
                width,
                mix,
            }) => mk(
                RuntimeEffect::Reverb(ReverbEffect::from_state(
                    room_size.clone(),
                    damping.clone(),
                    width.clone(),
                    mix.clone(),
                    sample_rate,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = ReverbEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Reverb(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::NoiseSuppressor(d) => match registry.controls.get(node_id) {
            Some(EffectControl::NoiseSuppressor { controls }) => mk(
                RuntimeEffect::NoiseSuppressor(NoiseSuppressorEffect::from_state(
                    controls.clone(),
                    sample_rate,
                    realtime,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = NoiseSuppressorEffect::new(d, sample_rate, realtime);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(
                    RuntimeEffect::NoiseSuppressor(e),
                    Some(c),
                    None,
                    None,
                    None,
                    None,
                )
            }
        },
        EffectSpec::Declick(d) => match registry.controls.get(node_id) {
            Some(EffectControl::Declick {
                sensitivity,
                max_width_ms,
            }) => mk(
                RuntimeEffect::Declick(DeclickEffect::from_state(
                    sensitivity.clone(),
                    max_width_ms.clone(),
                    sample_rate,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = DeclickEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::Declick(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::DeEsser(d) => match registry.controls.get(node_id) {
            Some(EffectControl::DeEsser {
                frequency,
                threshold_db,
                ratio,
            }) => mk(
                RuntimeEffect::DeEsser(DeEsserEffect::from_state(
                    frequency.clone(),
                    threshold_db.clone(),
                    ratio.clone(),
                    sample_rate,
                )),
                None,
                None,
                None,
                None,
                None,
            ),
            _ => {
                let (e, c) = DeEsserEffect::new(d, sample_rate);
                registry.controls.insert(node_id.to_string(), c.clone());
                mk(RuntimeEffect::DeEsser(e), Some(c), None, None, None, None)
            }
        },
        EffectSpec::Plugin {
            format,
            ref path,
            ref plugin_id,
            ref state,
            ..
        } => {
            // Empty path == node not yet configured: inert passthrough, not a
            // failure. Silence is reserved for a real load error below.
            if path.is_empty() {
                crate::audio::plugins::registry::forget(node_id);
                let muted = Arc::new(AtomicBool::new(false));
                return mk(
                    RuntimeEffect::Mute(MuteEffect::from_state(muted)),
                    None,
                    None,
                    None,
                    None,
                    None,
                );
            }
            // Surfaced as a load failure rather than guessed at: a path without
            // a format is stored data we cannot act on.
            let Some(format) = format else {
                tracing::error!(node_id, path, "plugin node has no format");
                let muted = Arc::new(AtomicBool::new(true));
                return mk(
                    RuntimeEffect::Mute(MuteEffect::from_state(muted)),
                    None,
                    None,
                    None,
                    None,
                    None,
                );
            };
            // Claimed only once the instance actually exists (below): a burned
            // claim would leave the node with no editor target while the
            // previous plugin stayed installed behind it.
            let primary = primary && !registry.plugin_primary_claimed.contains(node_id);
            // Every stereo pair shares the persistent per-node broadcast ring
            // (kept across rebuilds); each pair reads it through its own cursor,
            // so a UI write reaches all pairs, not just the first.
            let ring = registry
                .plugin_param_rings
                .entry(node_id.to_string())
                .or_insert_with(|| Arc::new(crate::audio::plugins::ParamRing::new()))
                .clone();
            let request = crate::audio::plugins::host_api::ActivateRequest {
                node_id,
                path,
                plugin_id,
                sample_rate,
                max_frames: PLUGIN_MAX_BLOCK,
                channels,
                state: state.as_deref(),
                primary,
                params: ring.clone(),
            };
            match crate::audio::plugins::registry::activate(format, request) {
                // Only the editor-target build publishes the control, so the UI
                // writes reach the audible instance.
                Ok(node) => {
                    if primary {
                        registry.plugin_primary_claimed.insert(node_id.to_string());
                    }
                    let control = primary.then_some(EffectControl::Plugin { events: ring });
                    // The plugin took the node whole, so the pipeline must stop
                    // splitting it into pairs and hand it every channel.
                    let full_width = node.channels() == channels;
                    let mut build = mk(
                        RuntimeEffect::HostedPlugin(HostedEffect::new(node, realtime)),
                        control,
                        None,
                        None,
                        None,
                        None,
                    );
                    build.full_width = full_width;
                    build
                }
                Err(e) => {
                    // Surface as silence, never a passthrough that hides the failure.
                    tracing::error!(node_id, path, plugin_id, error = %e, "plugin failed to load");
                    crate::audio::plugins::registry::forget(node_id);
                    let muted = Arc::new(AtomicBool::new(true));
                    mk(
                        RuntimeEffect::Mute(MuteEffect::from_state(muted)),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::util::{db_to_linear, load_f32};
    use super::*;
    use crate::audio::graph::{
        ChannelBalanceData, CompressorData, DeEsserData, DeclickData, DelayData, EqData, GainData,
        LevelMeterData, LimiterData, LufsMeterData, MuteData, NoiseGateData, NoiseSuppressorData,
        ReverbData, SaturatorData, SpectrumData, WaveformData,
    };
    use std::sync::atomic::Ordering;

    #[test]
    fn apply_update_gain_maps_db_to_linear() {
        let linear = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let c = EffectControl::Gain {
            linear: linear.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("gainDb".into(), serde_json::json!(-6.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert!((load_f32(&linear) - db_to_linear(-6.0)).abs() < 1e-6);
        // Unknown keys and wrong types are silently ignored.
        let mut bad = serde_json::Map::new();
        bad.insert("gain".into(), serde_json::json!(5.0));
        bad.insert("gainDb".into(), serde_json::json!("text"));
        c.apply_update(&serde_json::Value::Object(bad));
        assert_eq!(load_f32(&linear), db_to_linear(-6.0));
    }

    #[test]
    fn apply_update_mute_toggles_bool() {
        let muted = Arc::new(AtomicBool::new(false));
        let c = EffectControl::Mute {
            muted: muted.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("muted".into(), serde_json::json!(true));
        c.apply_update(&serde_json::Value::Object(m));
        assert!(muted.load(Ordering::Relaxed));
        // Missing key leaves state untouched.
        c.apply_update(&serde_json::Value::Object(serde_json::Map::new()));
        assert!(muted.load(Ordering::Relaxed));
        // Non-bool is ignored.
        let mut bad = serde_json::Map::new();
        bad.insert("muted".into(), serde_json::json!(1));
        c.apply_update(&serde_json::Value::Object(bad));
        assert!(muted.load(Ordering::Relaxed));
    }

    #[test]
    fn apply_update_channel_balance() {
        let left = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let right = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let c = EffectControl::ChannelBalance {
            left: left.clone(),
            right: right.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("leftGainDb".into(), serde_json::json!(-6.0));
        m.insert("rightGainDb".into(), serde_json::json!(6.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&left), db_to_linear(-6.0));
        assert_eq!(load_f32(&right), db_to_linear(6.0));
    }

    #[test]
    fn apply_update_saturator_clamps_ceiling_floor() {
        let ceiling = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let drive = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let c = EffectControl::Saturator {
            ceiling: ceiling.clone(),
            drive: drive.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("thresholdDb".into(), serde_json::json!(-120.0));
        m.insert("driveDb".into(), serde_json::json!(12.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&ceiling), db_to_linear(-120.0).max(1e-6));
        assert_eq!(load_f32(&drive), db_to_linear(12.0));
    }

    #[test]
    fn apply_update_eq_partial_array() {
        let gains: [Arc<AtomicU32>; 10] =
            std::array::from_fn(|_| Arc::new(AtomicU32::new(0.0f32.to_bits())));
        let c = EffectControl::Eq {
            gains: gains.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("gainsDb".into(), serde_json::json!([2.0, -2.0]));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&gains[0]), 2.0);
        assert_eq!(load_f32(&gains[1]), -2.0);
        assert_eq!(load_f32(&gains[2]), 0.0);
    }

    #[test]
    fn apply_update_limiter_clamps() {
        let ceiling = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let release_ms = Arc::new(AtomicU32::new(50.0f32.to_bits()));
        let c = EffectControl::Limiter {
            ceiling: ceiling.clone(),
            release_ms: release_ms.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("ceilingDb".into(), serde_json::json!(-200.0));
        m.insert("releaseMs".into(), serde_json::json!(0.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&ceiling), 1e-6);
        assert_eq!(load_f32(&release_ms), 0.1);
    }

    #[test]
    fn apply_update_compressor_clamps_each_param() {
        let threshold_db = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let ratio = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let attack_ms = Arc::new(AtomicU32::new(10.0f32.to_bits()));
        let release_ms = Arc::new(AtomicU32::new(100.0f32.to_bits()));
        let knee_db = Arc::new(AtomicU32::new(6.0f32.to_bits()));
        let makeup_db = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let c = EffectControl::Compressor {
            threshold_db: threshold_db.clone(),
            ratio: ratio.clone(),
            attack_ms: attack_ms.clone(),
            release_ms: release_ms.clone(),
            knee_db: knee_db.clone(),
            makeup_db: makeup_db.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("thresholdDb".into(), serde_json::json!(-24.0));
        m.insert("ratio".into(), serde_json::json!(0.5));
        m.insert("attackMs".into(), serde_json::json!(0.0));
        m.insert("releaseMs".into(), serde_json::json!(0.0));
        m.insert("kneeDb".into(), serde_json::json!(-5.0));
        m.insert("makeupDb".into(), serde_json::json!(7.5));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&threshold_db), -24.0);
        assert_eq!(load_f32(&ratio), 1.0);
        assert_eq!(load_f32(&attack_ms), 0.01);
        assert_eq!(load_f32(&release_ms), 0.1);
        assert_eq!(load_f32(&knee_db), 0.0);
        assert_eq!(load_f32(&makeup_db), 7.5);
    }

    #[test]
    fn apply_update_noise_gate_clamps() {
        let threshold_db = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let range_db = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let attack_ms = Arc::new(AtomicU32::new(10.0f32.to_bits()));
        let hold_ms = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let release_ms = Arc::new(AtomicU32::new(100.0f32.to_bits()));
        let c = EffectControl::NoiseGate {
            threshold_db: threshold_db.clone(),
            range_db: range_db.clone(),
            attack_ms: attack_ms.clone(),
            hold_ms: hold_ms.clone(),
            release_ms: release_ms.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("thresholdDb".into(), serde_json::json!(-40.0));
        m.insert("rangeDb".into(), serde_json::json!(5.0));
        m.insert("attackMs".into(), serde_json::json!(0.0));
        m.insert("holdMs".into(), serde_json::json!(-3.0));
        m.insert("releaseMs".into(), serde_json::json!(0.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&threshold_db), -40.0);
        assert_eq!(load_f32(&range_db), 0.0, "positive range clamps to 0");
        assert_eq!(load_f32(&attack_ms), 0.01);
        assert_eq!(load_f32(&hold_ms), 0.0);
        assert_eq!(load_f32(&release_ms), 0.1);
    }

    #[test]
    fn apply_update_delay_clamps() {
        let time_ms = Arc::new(AtomicU32::new(10.0f32.to_bits()));
        let feedback = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let mix = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let c = EffectControl::Delay {
            time_ms: time_ms.clone(),
            feedback: feedback.clone(),
            mix: mix.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("timeMs".into(), serde_json::json!(0.0));
        m.insert("feedback".into(), serde_json::json!(3.0));
        m.insert("mix".into(), serde_json::json!(-1.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&time_ms), 1.0);
        assert_eq!(load_f32(&feedback), 0.95);
        assert_eq!(load_f32(&mix), 0.0);
    }

    #[test]
    fn apply_update_reverb_clamps_all() {
        let room_size = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let damping = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let width = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let mix = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let c = EffectControl::Reverb {
            room_size: room_size.clone(),
            damping: damping.clone(),
            width: width.clone(),
            mix: mix.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("roomSize".into(), serde_json::json!(9.0));
        m.insert("damping".into(), serde_json::json!(-2.0));
        m.insert("width".into(), serde_json::json!(4.0));
        m.insert("mix".into(), serde_json::json!(-1.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&room_size), 1.0);
        assert_eq!(load_f32(&damping), 0.0);
        assert_eq!(load_f32(&width), 1.0);
        assert_eq!(load_f32(&mix), 0.0);
    }

    #[test]
    fn apply_update_noise_suppressor() {
        let c = EffectControl::NoiseSuppressor {
            controls: NoiseSuppressorControls {
                atten_lim_db: Arc::new(AtomicU32::new(0.0f32.to_bits())),
                pf_beta: Arc::new(AtomicU32::new(0.0f32.to_bits())),
                min_thresh_db: Arc::new(AtomicU32::new(0.0f32.to_bits())),
                max_erb_thresh_db: Arc::new(AtomicU32::new(0.0f32.to_bits())),
                max_df_thresh_db: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            },
        };
        let EffectControl::NoiseSuppressor { controls } = &c else {
            panic!("variant")
        };
        let mut m = serde_json::Map::new();
        m.insert("attenuationLimitDb".into(), serde_json::json!(-3.0));
        m.insert("postFilterBeta".into(), serde_json::json!(-1.0));
        m.insert("minThreshDb".into(), serde_json::json!(-12.0));
        m.insert("maxErbThreshDb".into(), serde_json::json!(25.0));
        m.insert("maxDfThreshDb".into(), serde_json::json!(15.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&controls.atten_lim_db), 0.0);
        assert_eq!(load_f32(&controls.pf_beta), 0.0);
        assert_eq!(load_f32(&controls.min_thresh_db), -12.0);
        assert_eq!(load_f32(&controls.max_erb_thresh_db), 25.0);
        assert_eq!(load_f32(&controls.max_df_thresh_db), 15.0);
    }

    #[test]
    fn apply_update_declick_and_de_esser_clamp() {
        let sensitivity = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let max_width_ms = Arc::new(AtomicU32::new(2.0f32.to_bits()));
        let c = EffectControl::Declick {
            sensitivity: sensitivity.clone(),
            max_width_ms: max_width_ms.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("sensitivity".into(), serde_json::json!(7.0));
        m.insert("maxWidthMs".into(), serde_json::json!(0.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&sensitivity), 1.0);
        assert_eq!(load_f32(&max_width_ms), 0.3);

        let frequency = Arc::new(AtomicU32::new(4000.0f32.to_bits()));
        let threshold_db = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let ratio = Arc::new(AtomicU32::new(2.0f32.to_bits()));
        let c = EffectControl::DeEsser {
            frequency: frequency.clone(),
            threshold_db: threshold_db.clone(),
            ratio: ratio.clone(),
        };
        let mut m = serde_json::Map::new();
        m.insert("frequency".into(), serde_json::json!(100.0));
        m.insert("thresholdDb".into(), serde_json::json!(5.0));
        m.insert("ratio".into(), serde_json::json!(50.0));
        c.apply_update(&serde_json::Value::Object(m));
        assert_eq!(load_f32(&frequency), 2000.0);
        assert_eq!(load_f32(&threshold_db), 0.0);
        assert_eq!(load_f32(&ratio), 12.0);
    }

    #[test]
    fn apply_update_plugin_pushes_params_to_ring() {
        let ring = Arc::new(crate::audio::plugins::ParamRing::new());
        let c = EffectControl::Plugin {
            events: ring.clone(),
        };
        // A fresh reader positioned before the writes sees them all.
        let mut cursor = ring.reader();
        let mut m = serde_json::Map::new();
        m.insert(
            "pluginParams".into(),
            serde_json::json!({"12": 0.5, "oops": 1.0, "abc": 2.0}),
        );
        c.apply_update(&serde_json::Value::Object(m));
        let mut seen = Vec::new();
        while let Some((id, v)) = ring.read(&mut cursor) {
            seen.push((id, v));
        }
        assert_eq!(seen, vec![(12, 0.5)], "non-numeric ids must be skipped");
    }

    #[test]
    fn instantiate_reuses_control_and_bypass_for_same_node() {
        let mut reg = EffectRegistry::new();
        let spec = EffectSpec::Gain(GainData {
            gain_db: -6.0,
            bypassed: true,
        });
        let first = instantiate_effect(&spec, "n1", 48_000, true, true, 2, &mut reg);
        assert!(first.bypass_is_new);
        assert!(first.control.is_some());
        assert!(
            first.bypass.load(Ordering::Relaxed),
            "bypassed spec latches"
        );
        let second = instantiate_effect(&spec, "n1", 48_000, true, true, 2, &mut reg);
        assert!(!second.bypass_is_new);
        assert!(second.control.is_none());
        let other = instantiate_effect(&spec, "n2", 48_000, true, true, 2, &mut reg);
        assert!(other.bypass_is_new);
        assert!(other.control.is_some());
    }

    #[test]
    fn limiter_rebuild_republishes_gr_atom() {
        let mut reg = EffectRegistry::new();
        let spec = EffectSpec::Limiter(LimiterData {
            ceiling_db: -6.0,
            lookahead_ms: 1.0,
            release_ms: 50.0,
            bypassed: false,
        });
        let first = instantiate_effect(&spec, "lim", 48_000, true, true, 2, &mut reg);
        let gr1 = first.gr.clone().expect("first build publishes GR");
        let second = instantiate_effect(&spec, "lim", 48_000, true, true, 2, &mut reg);
        let gr2 = second.gr.clone().expect("rebuild republishes GR");
        assert_eq!(gr1.node_id, gr2.node_id);
        assert!(Arc::ptr_eq(&gr1.gr_lin, &gr2.gr_lin), "same atom reused");
        assert!(second.control.is_none(), "control must not be re-published");
    }

    #[test]
    fn compressor_and_gate_publish_gr_handles() {
        let mut reg = EffectRegistry::new();
        let comp = instantiate_effect(
            &EffectSpec::Compressor(CompressorData {
                threshold_db: -12.0,
                ratio: 4.0,
                attack_ms: 5.0,
                release_ms: 100.0,
                knee_db: 0.0,
                makeup_db: 0.0,
                bypassed: false,
            }),
            "c",
            48_000,
            true,
            true,
            2,
            &mut reg,
        );
        assert!(comp.gr.is_some());
        let gate = instantiate_effect(
            &EffectSpec::NoiseGate(NoiseGateData {
                threshold_db: -30.0,
                range_db: -24.0,
                attack_ms: 5.0,
                hold_ms: 50.0,
                release_ms: 50.0,
                bypassed: false,
            }),
            "g",
            48_000,
            true,
            true,
            2,
            &mut reg,
        );
        assert!(gate.gr.is_some());
    }

    #[test]
    fn metering_specs_return_handles_once() {
        let mut reg = EffectRegistry::new();
        let lvl = instantiate_effect(
            &EffectSpec::LevelMeter(LevelMeterData {}),
            "m",
            48_000,
            true,
            true,
            2,
            &mut reg,
        );
        assert!(lvl.meter.is_some());
        let lvl2 = instantiate_effect(
            &EffectSpec::LevelMeter(LevelMeterData {}),
            "m",
            48_000,
            true,
            true,
            2,
            &mut reg,
        );
        assert!(lvl2.meter.is_none());

        let lufs = instantiate_effect(
            &EffectSpec::LufsMeter(LufsMeterData {}),
            "l",
            48_000,
            true,
            true,
            2,
            &mut reg,
        );
        assert!(lufs.lufs.is_some());
        let wave = instantiate_effect(
            &EffectSpec::Waveform(WaveformData {}),
            "w",
            48_000,
            true,
            true,
            2,
            &mut reg,
        );
        assert!(wave.scope.is_some());
        let spec_first = instantiate_effect(
            &EffectSpec::Spectrum(SpectrumData {}),
            "s",
            48_000,
            true,
            true,
            2,
            &mut reg,
        );
        assert!(spec_first.scope.is_some());
        let spec_second = instantiate_effect(
            &EffectSpec::Spectrum(SpectrumData {}),
            "s",
            48_000,
            true,
            true,
            2,
            &mut reg,
        );
        assert!(spec_second.scope.is_none());
    }

    #[test]
    fn plugin_without_path_is_inert_passthrough() {
        let mut reg = EffectRegistry::new();
        let spec = EffectSpec::Plugin {
            node_id: "p".into(),
            format: Some(crate::audio::plugins::PluginFormat::Vst3),
            path: String::new(),
            plugin_id: String::new(),
            bypassed: false,
            state: None,
        };
        let mut build = instantiate_effect(&spec, "p", 48_000, true, true, 2, &mut reg);
        assert!(build.control.is_none());
        assert!(!build.full_width);
        let mut buf = vec![0.3f32, -0.4];
        build.effect.process_with_sidechain(&mut buf, None, 1);
        assert_eq!(buf, vec![0.3, -0.4]);
    }

    #[test]
    fn plugin_without_format_is_silenced() {
        let mut reg = EffectRegistry::new();
        let spec = EffectSpec::Plugin {
            node_id: "p2".into(),
            format: None,
            path: "/some/plugin.vst3".into(),
            plugin_id: String::new(),
            bypassed: false,
            state: None,
        };
        let mut build = instantiate_effect(&spec, "p2", 48_000, true, true, 2, &mut reg);
        let mut buf = vec![0.3f32, -0.4];
        build.effect.process_with_sidechain(&mut buf, None, 1);
        assert_eq!(buf, vec![0.0, 0.0]);
    }

    #[test]
    fn plugin_with_unloadable_path_is_silenced() {
        let mut reg = EffectRegistry::new();
        let spec = EffectSpec::Plugin {
            node_id: "p3".into(),
            format: Some(crate::audio::plugins::PluginFormat::Vst3),
            path: "/definitely/not/a/plugin.vst3".into(),
            plugin_id: String::new(),
            bypassed: false,
            state: None,
        };
        let mut build = instantiate_effect(&spec, "p3", 48_000, true, true, 2, &mut reg);
        let mut buf = vec![0.3f32, -0.4];
        build.effect.process_with_sidechain(&mut buf, None, 1);
        assert_eq!(buf, vec![0.0, 0.0]);
    }

    #[test]
    fn begin_reconcile_resets_primary_claims() {
        let mut reg = EffectRegistry::new();
        reg.plugin_primary_claimed.insert("x".to_string());
        assert!(reg.plugin_primary_claimed.contains("x"));
        reg.begin_reconcile();
        assert!(!reg.plugin_primary_claimed.contains("x"));
    }

    #[test]
    fn runtime_effect_dispatch_latency_and_processing() {
        let mut reg = EffectRegistry::new();
        let sr = 48_000;
        let specs: Vec<EffectSpec> = vec![
            EffectSpec::Gain(GainData {
                gain_db: -6.0,
                bypassed: false,
            }),
            EffectSpec::Mute(MuteData {
                muted: false,
                bypassed: false,
            }),
            EffectSpec::ChannelBalance(ChannelBalanceData {
                left_gain_db: 0.0,
                right_gain_db: 0.0,
                bypassed: false,
            }),
            EffectSpec::Saturator(SaturatorData {
                threshold_db: 0.0,
                drive_db: 0.0,
                bypassed: false,
            }),
            EffectSpec::Eq(EqData {
                gains_db: [0.0; 10],
                bypassed: false,
            }),
            EffectSpec::LevelMeter(LevelMeterData {}),
            EffectSpec::LufsMeter(LufsMeterData {}),
            EffectSpec::Waveform(WaveformData {}),
            EffectSpec::Spectrum(SpectrumData {}),
            EffectSpec::Limiter(LimiterData {
                ceiling_db: 0.0,
                lookahead_ms: 1.0,
                release_ms: 50.0,
                bypassed: false,
            }),
            EffectSpec::Compressor(CompressorData {
                threshold_db: -12.0,
                ratio: 4.0,
                attack_ms: 5.0,
                release_ms: 100.0,
                knee_db: 0.0,
                makeup_db: 0.0,
                bypassed: false,
            }),
            EffectSpec::NoiseGate(NoiseGateData {
                threshold_db: -30.0,
                range_db: -24.0,
                attack_ms: 5.0,
                hold_ms: 0.0,
                release_ms: 50.0,
                bypassed: false,
            }),
            EffectSpec::Delay(DelayData {
                time_ms: 10.0,
                feedback: 0.0,
                mix: 0.5,
                bypassed: false,
            }),
            EffectSpec::Reverb(ReverbData {
                room_size: 0.5,
                damping: 0.5,
                width: 1.0,
                mix: 0.5,
                bypassed: false,
            }),
            EffectSpec::Declick(DeclickData {
                sensitivity: 0.5,
                max_width_ms: 2.0,
                bypassed: false,
            }),
            EffectSpec::DeEsser(DeEsserData {
                frequency: 6000.0,
                threshold_db: -20.0,
                ratio: 4.0,
                bypassed: false,
            }),
        ];
        for spec in &specs {
            let mut build = instantiate_effect(spec, "dispatch", sr, true, true, 2, &mut reg);
            assert!(build.effect.latency_frames() < 100_000);
            let mut buf = vec![0.25f32; 96];
            let side = vec![0.5f32; 96];
            build
                .effect
                .process_with_sidechain(&mut buf, Some(&side), 48);
            assert!(buf.iter().all(|s| s.is_finite()));
            reg.controls.clear();
            reg.meters.clear();
            reg.lufs.clear();
            reg.scopes.clear();
            reg.gr_atomics.clear();
            reg.plugin_primary_claimed.clear();
        }
    }

    #[test]
    fn runtime_effects_contain_non_finite_samples_and_recover() {
        let mut reg = EffectRegistry::new();
        let specs = [
            EffectSpec::Gain(GainData {
                gain_db: 0.0,
                bypassed: false,
            }),
            EffectSpec::Saturator(SaturatorData {
                threshold_db: -1.0,
                drive_db: 3.0,
                bypassed: false,
            }),
            EffectSpec::Eq(EqData {
                gains_db: [0.0; 10],
                bypassed: false,
            }),
            EffectSpec::Limiter(LimiterData {
                ceiling_db: -1.0,
                lookahead_ms: 1.0,
                release_ms: 50.0,
                bypassed: false,
            }),
            EffectSpec::Delay(DelayData {
                time_ms: 10.0,
                feedback: 0.5,
                mix: 0.5,
                bypassed: false,
            }),
            EffectSpec::Reverb(ReverbData {
                room_size: 0.5,
                damping: 0.5,
                width: 1.0,
                mix: 0.5,
                bypassed: false,
            }),
        ];
        for spec in &specs {
            let mut build = instantiate_effect(spec, "finite", 48_000, true, true, 2, &mut reg);
            let mut poisoned = vec![0.25; 96];
            poisoned[10] = f32::NAN;
            poisoned[11] = f32::INFINITY;
            build.effect.process_with_sidechain(&mut poisoned, None, 48);
            assert!(poisoned.iter().all(|sample| sample.is_finite()), "{spec:?}");

            let mut clean = vec![0.25; 96];
            build.effect.process_with_sidechain(&mut clean, None, 48);
            assert!(
                clean.iter().all(|sample| sample.is_finite()),
                "state poisoned: {spec:?}"
            );
            reg.controls.clear();
            reg.gr_atomics.clear();
        }
    }

    #[test]
    fn rebuild_uses_from_state_and_shares_atoms() {
        let mut reg = EffectRegistry::new();
        let sr = 48_000;
        let specs: Vec<EffectSpec> = vec![
            EffectSpec::Gain(GainData {
                gain_db: -6.0,
                bypassed: false,
            }),
            EffectSpec::Mute(MuteData {
                muted: false,
                bypassed: false,
            }),
            EffectSpec::ChannelBalance(ChannelBalanceData {
                left_gain_db: -3.0,
                right_gain_db: 3.0,
                bypassed: false,
            }),
            EffectSpec::Saturator(SaturatorData {
                threshold_db: 0.0,
                drive_db: 3.0,
                bypassed: false,
            }),
            EffectSpec::Eq(EqData {
                gains_db: [1.0; 10],
                bypassed: false,
            }),
            EffectSpec::LevelMeter(LevelMeterData {}),
            EffectSpec::LufsMeter(LufsMeterData {}),
            EffectSpec::Waveform(WaveformData {}),
            EffectSpec::Spectrum(SpectrumData {}),
            EffectSpec::Limiter(LimiterData {
                ceiling_db: -6.0,
                lookahead_ms: 1.0,
                release_ms: 50.0,
                bypassed: false,
            }),
            EffectSpec::Compressor(CompressorData {
                threshold_db: -12.0,
                ratio: 4.0,
                attack_ms: 5.0,
                release_ms: 100.0,
                knee_db: 0.0,
                makeup_db: 0.0,
                bypassed: false,
            }),
            EffectSpec::NoiseGate(NoiseGateData {
                threshold_db: -30.0,
                range_db: -24.0,
                attack_ms: 5.0,
                hold_ms: 0.0,
                release_ms: 50.0,
                bypassed: false,
            }),
            EffectSpec::Delay(DelayData {
                time_ms: 10.0,
                feedback: 0.3,
                mix: 0.5,
                bypassed: false,
            }),
            EffectSpec::Reverb(ReverbData {
                room_size: 0.5,
                damping: 0.5,
                width: 1.0,
                mix: 0.5,
                bypassed: false,
            }),
            EffectSpec::NoiseSuppressor(NoiseSuppressorData {
                attenuation_limit_db: 15.0,
                post_filter_beta: 0.0,
                min_thresh_db: -10.0,
                max_erb_thresh_db: 30.0,
                max_df_thresh_db: 20.0,
                bypassed: false,
            }),
            EffectSpec::Declick(DeclickData {
                sensitivity: 0.5,
                max_width_ms: 2.0,
                bypassed: false,
            }),
            EffectSpec::DeEsser(DeEsserData {
                frequency: 6000.0,
                threshold_db: -20.0,
                ratio: 4.0,
                bypassed: false,
            }),
        ];
        for (i, spec) in specs.iter().enumerate() {
            let node = format!("reuse-{i}");
            let first = instantiate_effect(spec, &node, sr, false, true, 2, &mut reg);
            assert!(
                first.control.is_some()
                    || first.meter.is_some()
                    || first.lufs.is_some()
                    || first.scope.is_some()
            );
            // Same node id → the registry already holds the state; the rebuild
            // must go through from_state and stay functional.
            let mut second = instantiate_effect(spec, &node, sr, false, true, 2, &mut reg);
            assert!(second.control.is_none());
            assert!(second.meter.is_none() && second.lufs.is_none() && second.scope.is_none());
            let mut buf = vec![0.3f32; 96];
            second.effect.process_with_sidechain(&mut buf, None, 48);
            assert!(buf.iter().all(|s| s.is_finite()));
        }
    }
}
