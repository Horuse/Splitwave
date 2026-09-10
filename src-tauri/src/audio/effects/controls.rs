use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::Value;

use super::noise_suppressor::NoiseSuppressorControls;
use super::util::{db_to_linear, num, store_f32};

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
