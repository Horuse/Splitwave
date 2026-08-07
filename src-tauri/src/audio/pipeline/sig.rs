use std::collections::HashSet;

use crate::audio::graph::{EdgeKind, EffectSpec, InputSpec, OutputSpec, ValidGraph};

/// Sentinel id used for the monitor-mode pseudo-output (graphs with no
/// real outputs but at least one analyzer). Lets us key it the same way
/// as real outputs in `bridges_by_output` / `output_sig` maps.
pub(super) const MONITOR_KEY: &str = "__monitor__";

/// Canonical view of an output's sub-graph for diffing across reconciles.
/// Equal `OutputSig`s mean the output's worker can keep running with
/// exactly its current effect chain, sources, and consumer rings.
#[derive(PartialEq, Clone)]
pub(super) struct OutputSig {
    /// `None` only for the monitor pseudo-output.
    pub output_spec: Option<OutputSpec>,
    /// Reachable inputs (id + spec), sorted by id.
    pub inputs: Vec<(String, InputSpec)>,
    /// Reachable effects (id + spec), sorted by id.
    pub effects: Vec<(String, EffectSpec)>,
    /// Edges with both endpoints in the sub-graph, sorted.
    pub edges: Vec<(String, String, EdgeKind)>,
}

pub(super) fn compute_output_sig(graph: &ValidGraph, output_id: &str) -> OutputSig {
    let reachable: HashSet<String> = if output_id == MONITOR_KEY {
        let mut all = HashSet::new();
        for inp in &graph.inputs {
            all.insert(inp.id.clone());
        }
        for eff in &graph.effects {
            all.insert(eff.id.clone());
        }
        all
    } else {
        super::dag::reachable_backward(output_id, graph)
    };

    let output_spec = if output_id == MONITOR_KEY {
        None
    } else {
        graph
            .outputs
            .iter()
            .find(|o| o.id == output_id)
            .map(|o| o.spec.clone())
    };

    let mut inputs: Vec<(String, InputSpec)> = graph
        .inputs
        .iter()
        .filter(|i| reachable.contains(&i.id))
        .map(|i| (i.id.clone(), i.spec.clone()))
        .collect();
    inputs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut effects: Vec<(String, EffectSpec)> = graph
        .effects
        .iter()
        .filter(|e| reachable.contains(&e.id))
        .map(|e| (e.id.clone(), structural_effect(&e.spec)))
        .collect();
    effects.sort_by(|a, b| a.0.cmp(&b.0));

    let mut edges: Vec<(String, String, EdgeKind)> = graph
        .edges
        .iter()
        .filter(|e| reachable.contains(&e.from) && (reachable.contains(&e.to) || e.to == output_id))
        .map(|e| (e.from.clone(), e.to.clone(), e.kind))
        .collect();
    edges.sort_by(|a, b| {
        let ord = a.0.cmp(&b.0);
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
        let ord = a.1.cmp(&b.1);
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
        edge_kind_ord(a.2).cmp(&edge_kind_ord(b.2))
    });

    OutputSig {
        output_spec,
        inputs,
        effects,
        edges,
    }
}

/// Effect spec with live DSP params zeroed, leaving only fields that require a
/// worker rebuild. Live params reach the running effect via `update_effect`, so
/// two specs differing only in them compare equal here and skip reconcile.
///
/// A field is structural unless zeroed below, so an unhandled field forces a
/// rebuild rather than being silently dropped from the signature.
fn structural_effect(spec: &EffectSpec) -> EffectSpec {
    use EffectSpec as E;
    let mut s = spec.clone();
    match &mut s {
        E::Gain(d) => {
            d.gain_db = 0.0;
            d.bypassed = false;
        }
        E::Mute(d) => {
            d.muted = false;
            d.bypassed = false;
        }
        E::ChannelBalance(d) => {
            d.left_gain_db = 0.0;
            d.right_gain_db = 0.0;
            d.bypassed = false;
        }
        E::Saturator(d) => {
            d.threshold_db = 0.0;
            d.drive_db = 0.0;
            d.bypassed = false;
        }
        E::Eq(d) => {
            d.gains_db = [0.0; 10];
            d.bypassed = false;
        }
        E::Limiter(d) => {
            // lookahead_ms sizes the delay line at build time -- keep it structural.
            d.ceiling_db = 0.0;
            d.release_ms = 0.0;
            d.bypassed = false;
        }
        E::Compressor(d) => {
            d.threshold_db = 0.0;
            d.ratio = 0.0;
            d.attack_ms = 0.0;
            d.release_ms = 0.0;
            d.knee_db = 0.0;
            d.makeup_db = 0.0;
            d.bypassed = false;
        }
        E::NoiseGate(d) => {
            d.threshold_db = 0.0;
            d.range_db = 0.0;
            d.attack_ms = 0.0;
            d.hold_ms = 0.0;
            d.release_ms = 0.0;
            d.bypassed = false;
        }
        E::Delay(d) => {
            d.time_ms = 0.0;
            d.feedback = 0.0;
            d.mix = 0.0;
            d.bypassed = false;
        }
        E::Reverb(d) => {
            d.room_size = 0.0;
            d.damping = 0.0;
            d.width = 0.0;
            d.mix = 0.0;
            d.bypassed = false;
        }
        E::NoiseSuppressor(d) => {
            d.attenuation_limit_db = 0.0;
            d.post_filter_beta = 0.0;
            d.min_thresh_db = 0.0;
            d.max_erb_thresh_db = 0.0;
            d.max_df_thresh_db = 0.0;
            d.bypassed = false;
        }
        E::Declick(d) => {
            d.sensitivity = 0.0;
            d.max_width_ms = 0.0;
            d.bypassed = false;
        }
        E::DeEsser(d) => {
            d.frequency = 0.0;
            d.threshold_db = 0.0;
            d.ratio = 0.0;
            d.bypassed = false;
        }
        // path / plugin_id are structural; bypass rides its own atomic, and
        // state is applied only at instantiation so it must not force a rebuild.
        E::Plugin {
            bypassed, state, ..
        } => {
            *bypassed = false;
            *state = None;
        }
        // No live params (or all fields structural): compared as-is.
        E::LevelMeter(_) | E::LufsMeter(_) | E::Waveform(_) | E::Spectrum(_) => {}
    }
    s
}

#[inline]
fn edge_kind_ord(k: EdgeKind) -> u8 {
    match k {
        EdgeKind::Main => 0,
        EdgeKind::Sidechain => 1,
    }
}
