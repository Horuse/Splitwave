use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;

use super::controls::EffectControl;
use super::level_meter::MeterHandle;
use super::lufs_meter::LufsHandle;
use super::waveform::WaveformHandle;
use super::RuntimeEffect;

/// Shared atom for a dynamic-gain effect (compressor / noise gate / limiter).
/// The audio thread writes the block's minimum gain each block; the meter tick
/// thread reads it and emits `audio://gr` to the frontend.
#[derive(Clone)]
pub struct GrHandle {
    pub node_id: String,
    pub gr_lin: Arc<AtomicU32>,
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
    pub(super) controls: HashMap<String, EffectControl>,
    pub(super) bypasses: HashMap<String, Arc<AtomicBool>>,
    pub(super) meters: HashMap<String, MeterHandle>,
    pub(super) lufs: HashMap<String, LufsHandle>,
    pub(super) gr_atomics: HashMap<String, Arc<AtomicU32>>,
    pub(super) scopes: HashMap<String, WaveformHandle>,
    // Per-plugin UI->RT parameter queue, reused across rebuilds so the control
    // handed to the frontend keeps reaching the current `PluginNode`.
    pub(super) plugin_param_rings: HashMap<String, Arc<crate::audio::plugins::ParamRing>>,
    // Plugin node ids that already claimed the editor-target (primary) instance
    // in the current reconcile. A node feeding several real outputs is built
    // once per output; only the first claim owns the editor, the rest are
    // metering/duplicate instances parked in the graveyard.
    pub(super) plugin_primary_claimed: HashSet<String>,
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
