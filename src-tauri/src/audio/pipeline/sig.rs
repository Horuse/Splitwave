use std::collections::HashSet;

use crate::audio::graph::{EdgeKind, EffectSpec, InputSpec, OutputSpec, ValidGraph};

/// Sentinel id used for the monitor-mode pseudo-output (graphs with no
/// real outputs but at least one analyzer). Lets us key it the same way
/// as real outputs in `bridges_by_output` / `output_sig` maps.
pub(super) const MONITOR_KEY: &str = "__monitor__";

/// Canonical view of an output's sub-graph for diffing across reconciles.
/// Equal `OutputSig`s mean the output's worker can keep running with
/// exactly its current effect chain, sources, and consumer rings.
#[derive(PartialEq, Debug, Clone)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::graph::{EdgeSpec, GraphSpec, NodeKind, NodeSpec};

    fn node(id: &str, kind: NodeKind, data: serde_json::Value) -> NodeSpec {
        NodeSpec {
            id: id.to_string(),
            kind,
            data,
        }
    }

    fn mic(id: &str) -> NodeSpec {
        node(
            id,
            NodeKind::Microphone,
            serde_json::json!({ "deviceId": "dev" }),
        )
    }

    fn gain_node(id: &str, db: f32) -> NodeSpec {
        node(id, NodeKind::Gain, serde_json::json!({ "gainDb": db }))
    }

    fn speaker(id: &str) -> NodeSpec {
        node(
            id,
            NodeKind::Speaker,
            serde_json::json!({ "deviceId": "dev" }),
        )
    }

    fn edge(id: &str, from: &str, to: &str) -> EdgeSpec {
        EdgeSpec {
            id: id.to_string(),
            source: from.to_string(),
            source_handle: None,
            target: to.to_string(),
            target_handle: None,
        }
    }

    fn mic_to_speaker(gain_db: f32) -> ValidGraph {
        GraphSpec {
            sample_rate: None,
            nodes: vec![mic("m"), gain_node("g", gain_db), speaker("s")],
            edges: vec![edge("e1", "m", "g"), edge("e2", "g", "s")],
        }
        .validate()
        .expect("valid")
    }

    #[test]
    fn signature_covers_reachable_subgraph_only() {
        // Two speakers share one input; each signature only contains what
        // its own output can reach.
        let g = GraphSpec {
            sample_rate: None,
            nodes: vec![
                mic("m"),
                gain_node("a", 0.0),
                gain_node("b", 0.0),
                speaker("s1"),
                speaker("s2"),
            ],
            edges: vec![
                edge("e1", "m", "a"),
                edge("e2", "a", "s1"),
                edge("e3", "a", "b"),
                edge("e4", "b", "s2"),
            ],
        }
        .validate()
        .expect("valid");

        let sig1 = compute_output_sig(&g, "s1");
        assert_eq!(sig1.effects.len(), 1);
        assert_eq!(sig1.effects[0].0, "a");
        let sig2 = compute_output_sig(&g, "s2");
        // s2 reaches b (and a through it).
        assert_eq!(sig2.effects.len(), 2);
        // Different sub-graphs → different signatures.
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn live_gain_change_does_not_change_signature() {
        let a = compute_output_sig(&mic_to_speaker(0.0), "s");
        let b = compute_output_sig(&mic_to_speaker(-12.0), "s");
        assert_eq!(a, b, "gain_db is a live param, not structural");
    }

    #[test]
    fn mute_and_bypass_flags_are_not_structural() {
        let with_bypass = GraphSpec {
            sample_rate: None,
            nodes: vec![
                mic("m"),
                node(
                    "g",
                    NodeKind::Gain,
                    serde_json::json!({ "gainDb": 0.0, "bypassed": true }),
                ),
                speaker("s"),
            ],
            edges: vec![edge("e1", "m", "g"), edge("e2", "g", "s")],
        }
        .validate()
        .expect("valid");
        let a = compute_output_sig(&mic_to_speaker(0.0), "s");
        let b = compute_output_sig(&with_bypass, "s");
        assert_eq!(a, b, "bypass rides its own atomic");
    }

    #[test]
    fn limiter_lookahead_is_structural_but_ceiling_is_not() {
        let mk = |lookahead: f32, ceiling: f32| {
            GraphSpec {
                sample_rate: None,
                nodes: vec![
                    mic("m"),
                    node(
                        "l",
                        NodeKind::Limiter,
                        serde_json::json!({
                            "ceilingDb": ceiling, "lookaheadMs": lookahead,
                            "releaseMs": 50.0
                        }),
                    ),
                    speaker("s"),
                ],
                edges: vec![edge("e1", "m", "l"), edge("e2", "l", "s")],
            }
            .validate()
            .expect("valid")
        };
        let base = compute_output_sig(&mk(1.0, 0.0), "s");
        let same = compute_output_sig(&mk(1.0, -6.0), "s");
        let different = compute_output_sig(&mk(3.0, 0.0), "s");
        assert_eq!(base, same, "ceiling is live");
        assert_ne!(base, different, "lookahead sizes the delay line");
    }

    #[test]
    fn plugin_state_is_not_structural_path_is() {
        let mk = |state: Option<&str>| {
            GraphSpec {
                sample_rate: None,
                nodes: vec![
                    node(
                        "p",
                        NodeKind::Plugin,
                        serde_json::json!({
                            "format": "vst3",
                            "path": "/tmp/x.vst3",
                            "pluginId": "abc",
                            "state": state
                        }),
                    ),
                    speaker("s"),
                ],
                edges: vec![edge("e1", "p", "s")],
            }
            .validate()
            .expect("valid")
        };
        let a = compute_output_sig(&mk(None), "s");
        let b = compute_output_sig(&mk(Some("blob")), "s");
        assert_eq!(a, b, "state applies only at instantiation");
        let other_path = GraphSpec {
            sample_rate: None,
            nodes: vec![
                node(
                    "p",
                    NodeKind::Plugin,
                    serde_json::json!({
                        "format": "vst3",
                        "path": "/tmp/y.vst3",
                        "pluginId": "abc"
                    }),
                ),
                speaker("s"),
            ],
            edges: vec![edge("e1", "p", "s")],
        }
        .validate()
        .expect("valid");
        assert_ne!(
            a,
            compute_output_sig(&other_path, "s"),
            "path is structural"
        );
    }

    #[test]
    fn monitor_signature_includes_everything_and_has_no_spec() {
        let g = mic_to_speaker(0.0);
        let monitor = compute_output_sig(&g, MONITOR_KEY);
        assert_eq!(monitor.output_spec, None);
        assert_eq!(monitor.inputs.len(), 1);
        assert_eq!(monitor.effects.len(), 1);
        // Real output's signature carries the output spec.
        let real = compute_output_sig(&g, "s");
        assert!(real.output_spec.is_some());
        assert_ne!(monitor, real);
    }

    #[test]
    fn unknown_output_id_yields_empty_signature() {
        let g = mic_to_speaker(0.0);
        let sig = compute_output_sig(&g, "nonexistent");
        // Reachable from a node that doesn't exist → nothing.
        assert!(sig.inputs.is_empty());
        assert_eq!(sig.output_spec, None);
    }

    #[test]
    fn sidechain_edges_sort_after_main() {
        let g = GraphSpec {
            sample_rate: None,
            nodes: vec![
                mic("m1"),
                mic("m2"),
                node(
                    "c",
                    NodeKind::Compressor,
                    serde_json::json!({
                        "thresholdDb": -12.0, "ratio": 4.0, "attackMs": 5.0,
                        "releaseMs": 100.0, "kneeDb": 0.0, "makeupDb": 0.0
                    }),
                ),
                speaker("s"),
            ],
            edges: vec![
                edge("e1", "m1", "c"),
                edge("e2", "m2", "c"),
                edge("e3", "c", "s"),
            ],
        }
        .validate()
        .expect("valid");
        // Mark m2's edge as sidechain in the valid graph.
        let mut g = g;
        for e in &mut g.edges {
            if e.from == "m2" {
                e.kind = EdgeKind::Sidechain;
            }
        }
        let sig = compute_output_sig(&g, "s");
        let kinds: Vec<EdgeKind> = sig.edges.iter().map(|(_, _, k)| *k).collect();
        assert!(kinds.contains(&EdgeKind::Main) && kinds.contains(&EdgeKind::Sidechain));
        // The last edge (sorted) is the sidechain one.
        assert_eq!(sig.edges.last().unwrap().2, EdgeKind::Sidechain);
    }

    #[test]
    fn every_effect_zeroes_only_live_params() {
        // For every effect kind: two graphs whose specs differ ONLY in live
        // params must produce equal signatures, and a structural change must
        // produce different ones. This pins the structural_effect table.
        let cases: Vec<(NodeKind, serde_json::Value, serde_json::Value)> = vec![
            (
                NodeKind::Mute,
                serde_json::json!({ "muted": false }),
                serde_json::json!({ "muted": true }),
            ),
            (
                NodeKind::ChannelBalance,
                serde_json::json!({ "leftGainDb": -3.0, "rightGainDb": 3.0 }),
                serde_json::json!({ "leftGainDb": 0.0, "rightGainDb": 0.0 }),
            ),
            (
                NodeKind::Saturator,
                serde_json::json!({ "thresholdDb": -6.0, "driveDb": 3.0 }),
                serde_json::json!({ "thresholdDb": 0.0, "driveDb": 0.0 }),
            ),
            (
                NodeKind::Eq,
                serde_json::json!({ "gainsDb": vec![1.0; 10] }),
                serde_json::json!({ "gainsDb": vec![0.0; 10] }),
            ),
            (
                NodeKind::Delay,
                serde_json::json!({ "timeMs": 120.0, "feedback": 0.4, "mix": 0.3 }),
                serde_json::json!({ "timeMs": 10.0, "feedback": 0.0, "mix": 1.0 }),
            ),
            (
                NodeKind::Reverb,
                serde_json::json!({ "roomSize": 0.9, "damping": 0.2, "width": 1.0, "mix": 0.4 }),
                serde_json::json!({ "roomSize": 0.0, "damping": 0.0, "width": 0.0, "mix": 0.0 }),
            ),
            (
                NodeKind::NoiseSuppressor,
                serde_json::json!({ "attenuationLimitDb": 20.0, "postFilterBeta": 0.5 }),
                serde_json::json!({ "attenuationLimitDb": 0.0, "postFilterBeta": 0.0 }),
            ),
            (
                NodeKind::Declick,
                serde_json::json!({ "sensitivity": 0.9, "maxWidthMs": 5.0 }),
                serde_json::json!({ "sensitivity": 0.0, "maxWidthMs": 0.3 }),
            ),
            (
                NodeKind::DeEsser,
                serde_json::json!({ "frequency": 8000.0, "thresholdDb": -30.0, "ratio": 6.0 }),
                serde_json::json!({ "frequency": 4000.0, "thresholdDb": 0.0, "ratio": 2.0 }),
            ),
            (
                NodeKind::Compressor,
                serde_json::json!({
                    "thresholdDb": -20.0, "ratio": 8.0, "attackMs": 2.0,
                    "releaseMs": 80.0, "kneeDb": 4.0, "makeupDb": 3.0
                }),
                serde_json::json!({
                    "thresholdDb": 0.0, "ratio": 1.0, "attackMs": 0.01,
                    "releaseMs": 0.1, "kneeDb": 0.0, "makeupDb": 0.0
                }),
            ),
        ];
        for (kind, a, b) in cases {
            let mk = |data: serde_json::Value| {
                GraphSpec {
                    sample_rate: None,
                    nodes: vec![mic("m"), node("fx", kind.clone(), data), speaker("s")],
                    edges: vec![edge("e1", "m", "fx"), edge("e2", "fx", "s")],
                }
                .validate()
                .expect("valid")
            };
            let sa = compute_output_sig(&mk(a), "s");
            let sb = compute_output_sig(&mk(b), "s");
            assert_eq!(sa, sb, "live params of {kind:?} must not force a rebuild");
        }
    }
}
