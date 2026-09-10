use super::*;

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

fn collab(id: &str) -> NodeSpec {
    node(
        id,
        NodeKind::WebRtcCollaborator,
        serde_json::json!({ "opusBitrate": 96_000, "opusApplication": "audio" }),
    )
}

fn speaker(id: &str) -> NodeSpec {
    node(
        id,
        NodeKind::Speaker,
        serde_json::json!({ "deviceId": "dev" }),
    )
}

fn edge(
    id: &str,
    source: &str,
    source_handle: Option<&str>,
    target: &str,
    target_handle: Option<&str>,
) -> EdgeSpec {
    EdgeSpec {
        id: id.to_string(),
        source: source.to_string(),
        source_handle: source_handle.map(str::to_string),
        target: target.to_string(),
        target_handle: target_handle.map(str::to_string),
    }
}

#[test]
fn send_only_collaborator_is_an_output() {
    let g = GraphSpec {
        sample_rate: None,
        nodes: vec![mic("m"), collab("w")],
        edges: vec![edge("e", "m", None, "w", Some("ch1"))],
    };
    let v = g.validate().expect("send-only graph is valid");
    assert_eq!(v.outputs.len(), 1);
    assert!(matches!(
        v.outputs[0].spec,
        OutputSpec::WebRtcSend { channels: 1, .. }
    ));
    assert_eq!(v.outputs[0].id, "w");
    assert_eq!(v.inputs.len(), 1);
    assert!(!v
        .inputs
        .iter()
        .any(|i| matches!(i.spec, InputSpec::WebRtcRecv { .. })));
}

#[test]
fn recv_only_collaborator_is_an_input() {
    let g = GraphSpec {
        sample_rate: None,
        nodes: vec![collab("w"), speaker("s")],
        edges: vec![edge("e", "w", Some("peer:p:0"), "s", None)],
    };
    let v = g.validate().expect("recv-only graph is valid");
    assert_eq!(v.inputs.len(), 1);
    assert_eq!(v.inputs[0].id, "w#recv");
    assert!(
        matches!(&v.inputs[0].spec, InputSpec::WebRtcRecv { node_id, .. } if node_id == "w")
    );
    assert!(!v
        .outputs
        .iter()
        .any(|o| matches!(o.spec, OutputSpec::WebRtcSend { .. })));
    assert_eq!(v.edges[0].from, "w#recv");
}

#[test]
fn duplex_collaborator_is_both() {
    let g = GraphSpec {
        sample_rate: None,
        nodes: vec![mic("m"), collab("w"), speaker("s")],
        edges: vec![
            edge("e1", "m", None, "w", Some("ch1")),
            edge("e2", "w", Some("peer:p:0"), "s", None),
        ],
    };
    let v = g.validate().expect("duplex graph is valid");
    assert!(v.outputs.iter().any(|o| o.id == "w"));
    assert!(v.inputs.iter().any(|i| i.id == "w#recv"));
    assert!(v.edges.iter().any(|e| e.to == "w"));
    assert!(v.edges.iter().any(|e| e.from == "w#recv"));
}

#[test]
fn unwired_collaborator_is_not_a_routing_error() {
    let g = GraphSpec {
        sample_rate: None,
        nodes: vec![collab("w")],
        edges: vec![],
    };
    let v = g
        .validate()
        .expect("an unwired collaborator is a destination in waiting");
    assert!(v.inputs.is_empty());
    assert!(v.outputs.is_empty());
}

#[test]
fn unrouted_output_is_valid_and_streams_silence() {
    let g = GraphSpec {
        sample_rate: None,
        nodes: vec![speaker("s")],
        edges: vec![],
    };
    let v = g.validate().expect("unrouted output is valid");
    assert!(v.inputs.is_empty());
    assert_eq!(v.outputs.len(), 1);
    assert_eq!(v.outputs[0].id, "s");
}

#[test]
fn effect_leading_to_output_survives_when_input_disconnects() {
    fn gain(id: &str) -> NodeSpec {
        node(id, NodeKind::Gain, serde_json::json!({ "gainDb": 0.0 }))
    }

    let g = GraphSpec {
        sample_rate: None,
        nodes: vec![gain("g"), speaker("s")],
        edges: vec![edge("e", "g", None, "s", None)],
    };
    let v = g
        .validate()
        .expect("effect + output without inputs is valid");
    assert!(v.inputs.is_empty());
    assert_eq!(v.effects.len(), 1);
    assert_eq!(v.effects[0].id, "g");
    assert_eq!(v.outputs.len(), 1);
    assert_eq!(v.outputs[0].id, "s");
    assert_eq!(v.edges.len(), 1);
}

#[test]
fn default_sample_rate_is_48000() {
    let g = GraphSpec {
        sample_rate: None,
        nodes: vec![speaker("s")],
        edges: vec![],
    };
    let v = g.validate().expect("graph valid");
    assert_eq!(v.sample_rate, 48_000);
}

#[test]
fn custom_sample_rate_is_preserved() {
    for sr in [44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 384_000] {
        let g = GraphSpec {
            sample_rate: Some(sr),
            nodes: vec![speaker("s")],
            edges: vec![],
        };
        let v = g.validate().expect("graph valid");
        assert_eq!(v.sample_rate, sr);
    }
}

#[test]
fn out_of_bounds_sample_rate_is_rejected() {
    for sr in [0, 4_000, 7_999, 384_001, 1_000_000] {
        let g = GraphSpec {
            sample_rate: Some(sr),
            nodes: vec![speaker("s")],
            edges: vec![],
        };
        assert!(g.validate().is_err());
    }
}
