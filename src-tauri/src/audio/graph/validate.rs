use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};

use serde::Deserialize;

use crate::error::{AppError, AppResult};

use super::types::*;

/// One node of the expanded graph. A dual-role UI node appears once per role it
/// plays, so `role` belongs to this entry rather than to `kind`.
struct RoleNode<'a> {
    id: String,
    kind: NodeKind,
    role: NodeCategory,
    data: &'a serde_json::Value,
}

/// Marks the receive half of a split dual-role node. Node ids are cuid2
/// (alphanumeric), so this can never collide with one.
pub(crate) const RECV_SUFFIX: &str = "#recv";

fn is_analyzer_kind(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::LevelMeter | NodeKind::LufsMeter | NodeKind::Waveform | NodeKind::Spectrum
    )
}

impl GraphSpec {
    /// Splits dual-role nodes so that every node below plays exactly one role.
    /// Each half of a WebRTC collaborator exists only if something is wired to
    /// that side: a send-only node opens no receive tap, and a receive-only node
    /// never clocks silence onto the wire.
    fn expand_roles(&self) -> (Vec<RoleNode<'_>>, Vec<EdgeSpec>) {
        let mut nodes: Vec<RoleNode<'_>> = Vec::with_capacity(self.nodes.len());
        for n in &self.nodes {
            if n.kind != NodeKind::WebRtcCollaborator {
                nodes.push(RoleNode {
                    id: n.id.clone(),
                    kind: n.kind,
                    role: n.kind.category(),
                    data: &n.data,
                });
                continue;
            }
            if self.edges.iter().any(|e| e.target == n.id) {
                nodes.push(RoleNode {
                    id: n.id.clone(),
                    kind: n.kind,
                    role: NodeCategory::Output,
                    data: &n.data,
                });
            }
            if self.edges.iter().any(|e| e.source == n.id) {
                nodes.push(RoleNode {
                    id: format!("{}{RECV_SUFFIX}", n.id),
                    kind: n.kind,
                    role: NodeCategory::Input,
                    data: &n.data,
                });
            }
        }

        // The send half keeps the original id, so edges into the node need no
        // rewrite; edges out of it now start at the receive half.
        let split: HashSet<&str> = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::WebRtcCollaborator)
            .map(|n| n.id.as_str())
            .collect();
        let edges = self
            .edges
            .iter()
            .map(|e| EdgeSpec {
                id: e.id.clone(),
                source: if split.contains(e.source.as_str()) {
                    format!("{}{RECV_SUFFIX}", e.source)
                } else {
                    e.source.clone()
                },
                source_handle: e.source_handle.clone(),
                target: e.target.clone(),
                target_handle: e.target_handle.clone(),
            })
            .collect();
        (nodes, edges)
    }

    /// Rules:
    /// - Inputs may fan out to many downstream nodes; if none, they're dropped.
    /// - Outputs may receive many incoming edges (mixed at the output).
    /// - Effects may have ≥1 incoming (act as a mixer-bus) and ≤1 outgoing.
    /// - Anything not on a path from some input to some output is dropped.
    /// - Cycles are rejected.
    pub fn validate(&self) -> AppResult<ValidGraph> {
        let (nodes, edges) = self.expand_roles();
        let nodes_by_id: HashMap<&str, &RoleNode> =
            nodes.iter().map(|n| (n.id.as_str(), n)).collect();

        let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &edges {
            if !nodes_by_id.contains_key(edge.source.as_str())
                || !nodes_by_id.contains_key(edge.target.as_str())
            {
                return Err(AppError::Validation(format!(
                    "edge {} references unknown node",
                    edge.id
                )));
            }
            // Edges into an input node make no sense — fail loudly.
            if let Some(n) = nodes_by_id.get(edge.target.as_str()) {
                if n.role == NodeCategory::Input {
                    return Err(AppError::Validation(format!(
                        "edge points into input node {:?}",
                        n.id
                    )));
                }
            }
            // Edges out of an output node likewise.
            if let Some(n) = nodes_by_id.get(edge.source.as_str()) {
                if n.role == NodeCategory::Output {
                    return Err(AppError::Validation(format!(
                        "edge starts from output node {:?}",
                        n.id
                    )));
                }
            }
            outgoing
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
            incoming
                .entry(edge.target.as_str())
                .or_default()
                .push(edge.source.as_str());
        }

        check_acyclic(&nodes, &outgoing)?;

        let has_destination = nodes
            .iter()
            .any(|n| n.role == NodeCategory::Output || is_analyzer_kind(n.kind))
            // A collaborator holds a live peer session from the moment it
            // exists, so an unwired one is a destination in waiting, not a
            // graph error.
            || self.nodes.iter().any(|n| n.kind == NodeKind::WebRtcCollaborator);
        if !has_destination {
            return Err(AppError::Validation(
                "no routing — connect an input to an output or a meter".into(),
            ));
        }

        let reachable_from_inputs = bfs_forward(&nodes, &outgoing, NodeCategory::Input);
        let reachable_from_terminals: HashSet<&str> = bfs_backward_pred(&nodes, &incoming, |n| {
            n.role == NodeCategory::Output || is_analyzer_kind(n.kind)
        });
        let routed: HashSet<&str> = reachable_from_inputs
            .intersection(&reachable_from_terminals)
            .copied()
            .collect();
        // Keep unrouted input nodes (so their capture + level meter run)
        // as well as nodes reachable from terminals (outputs, analyzers, and
        // their upstream effect chains, which stream silence if inputs disconnect).
        let mut keep = reachable_from_terminals;
        for n in &nodes {
            if n.role == NodeCategory::Input {
                keep.insert(n.id.as_str());
            }
        }

        let inputs = resolve_inputs(&nodes, &keep, &routed)?;
        let outputs = resolve_outputs(&nodes, &keep, &routed)?;
        let effects = resolve_effects(&nodes, &keep)?;

        let edges: Vec<ValidEdge> = edges
            .iter()
            .filter(|e| keep.contains(e.source.as_str()) && keep.contains(e.target.as_str()))
            .map(|e| ValidEdge {
                from: e.source.clone(),
                source_handle: e.source_handle.clone(),
                to: e.target.clone(),
                target_handle: e.target_handle.clone(),
                kind: match e.target_handle.as_deref() {
                    Some("sidechain") => EdgeKind::Sidechain,
                    _ => EdgeKind::Main,
                },
            })
            .collect();

        let sample_rate = match self.sample_rate {
            Some(sr) if !(8_000..=384_000).contains(&sr) => {
                return Err(AppError::Validation(format!(
                    "pipeline sample rate {sr} out of bounds (8000..=384000)"
                )));
            }
            Some(sr) => sr,
            None => 48_000,
        };

        Ok(ValidGraph {
            inputs,
            outputs,
            effects,
            edges,
            sample_rate,
        })
    }
}

/// `routed` are inputs on a real path to a terminal — they must resolve or
/// validation fails. `keep` may also include unrouted inputs (kept so their
/// capture + level meter run); if one of those fails to resolve (e.g. no
/// device selected yet) it's dropped silently rather than failing the graph.
fn resolve_inputs(
    nodes: &[RoleNode<'_>],
    keep: &HashSet<&str>,
    routed: &HashSet<&str>,
) -> AppResult<Vec<ValidInput>> {
    let mut result = Vec::new();
    for n in nodes {
        if n.role != NodeCategory::Input || !keep.contains(n.id.as_str()) {
            continue;
        }
        let resolved = (|| -> AppResult<(InputSpec, f32, bool)> {
            Ok(match n.kind {
                NodeKind::Microphone => {
                    let data: MicrophoneData = parse(n.data, "Microphone")?;
                    let spec = InputSpec::Microphone {
                        device_id: data
                            .device_id
                            .ok_or_else(|| miss(&n.id, "Microphone has no device selected"))?,
                    };
                    (spec, 1.0f32, true)
                }
                NodeKind::SystemAudio => {
                    let data: SystemAudioData = parse(n.data, "SystemAudio")?;
                    let spec = InputSpec::SystemAudio {
                        exclude_current_app: data.exclude_current_app,
                    };
                    (spec, data.volume, true)
                }
                NodeKind::AppAudio => {
                    let data: AppAudioData = parse(n.data, "AppAudio")?;
                    let spec = InputSpec::AppAudio {
                        bundle_id: data
                            .bundle_id
                            .ok_or_else(|| miss(&n.id, "App Audio has no application selected"))?,
                    };
                    (spec, data.volume, true)
                }
                NodeKind::AudioFile => {
                    let data: AudioFileData = parse(n.data, "AudioFile")?;
                    let spec = InputSpec::AudioFile {
                        file_path: data
                            .file_path
                            .ok_or_else(|| miss(&n.id, "Audio File has no file selected"))?,
                    };
                    (spec, data.volume, data.auto_start)
                }
                NodeKind::NetReceiver => {
                    let data: NetReceiverData = parse(n.data, "NetReceiver")?;
                    (InputSpec::NetReceiver { port: data.port }, 1.0f32, true)
                }
                // Receive half of a collaborator: the session is keyed by the
                // UI node, so the split suffix comes back off.
                NodeKind::WebRtcCollaborator => {
                    let data: WebRtcCollaboratorData = parse(n.data, "WebRtcCollaborator")?;
                    let spec = InputSpec::WebRtcRecv {
                        node_id: n.id.strip_suffix(RECV_SUFFIX).unwrap_or(&n.id).to_string(),
                        opus_bitrate: data.opus_bitrate,
                        opus_application: data.opus_application,
                    };
                    (spec, 1.0f32, true)
                }
                _ => unreachable!(),
            })
        })();
        let (spec, volume, auto_start) = match resolved {
            Ok(v) => v,
            Err(e) if routed.contains(n.id.as_str()) => return Err(e),
            Err(_) => continue,
        };
        result.push(ValidInput {
            id: n.id.clone(),
            spec,
            volume,
            auto_start,
        });
    }
    Ok(result)
}

fn resolve_outputs(
    nodes: &[RoleNode<'_>],
    keep: &HashSet<&str>,
    routed: &HashSet<&str>,
) -> AppResult<Vec<ValidOutput>> {
    let mut result = Vec::new();
    for n in nodes {
        if n.role != NodeCategory::Output || !keep.contains(n.id.as_str()) {
            continue;
        }
        let resolved = (|| -> AppResult<OutputSpec> {
            Ok(match n.kind {
                NodeKind::Speaker => {
                    let data: SpeakerData = parse(n.data, "Speaker")?;
                    OutputSpec::Speaker {
                        device_id: data
                            .device_id
                            .ok_or_else(|| miss(&n.id, "Speaker has no device selected"))?,
                    }
                }
                NodeKind::FileRecording => {
                    let data: FileRecordingData = parse(n.data, "FileRecording")?;
                    let file_path = data
                        .file_path
                        .ok_or_else(|| miss(&n.id, "File Recording has no path"))?;
                    let path = std::path::Path::new(&file_path);
                    let parent = path.parent().unwrap_or(std::path::Path::new("."));
                    if !parent.exists() {
                        return Err(choose_file_err(&n.id, "directory does not exist"));
                    }
                    match data.mode {
                        RecordingMode::New => {
                            if path.exists() {
                                return Err(choose_file_err(&n.id, "file already exists"));
                            }
                        }
                        RecordingMode::Overwrite => {}
                        RecordingMode::Append => {
                            if !matches!(
                                data.format,
                                RecordingFormat::Wav { .. } | RecordingFormat::Aiff { .. }
                            ) {
                                return Err(AppError::Validation(format!(
                                    "append recording is only supported for WAV/AIFF (node {})",
                                    n.id
                                )));
                            }
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    if matches!(data.format, RecordingFormat::Aac { .. }) {
                        return Err(AppError::Validation(format!(
                            "AAC recording is only supported on macOS (node {})",
                            n.id
                        )));
                    }
                    let max = data.format.max_channels();
                    if data.channels == 0 || data.channels > max {
                        return Err(AppError::Validation(format!(
                            "recording node {} asks for {} channels; format allows 1..{max}",
                            n.id, data.channels
                        )));
                    }
                    if let Some(sr) = data.sample_rate {
                        let max = if matches!(data.format, RecordingFormat::Flac { .. }) {
                            655_350
                        } else {
                            384_000
                        };
                        if !(8000..=max).contains(&sr) {
                            return Err(AppError::Validation(format!(
                                "recording node {} pins sample rate {sr}; expected 8000..{max}",
                                n.id
                            )));
                        }
                    }
                    OutputSpec::FileRecording {
                        file_path,
                        format: data.format,
                        channels: data.channels,
                        mode: data.mode,
                        sample_rate: data.sample_rate.filter(|_| {
                            !matches!(
                                data.format,
                                RecordingFormat::Opus { .. } | RecordingFormat::Mp3 { .. }
                            )
                        }),
                    }
                }
                NodeKind::NetSender => {
                    let data: NetSenderData = parse(n.data, "NetSender")?;
                    let ip: IpAddr = data
                        .target_ip
                        .trim()
                        .parse()
                        .map_err(|_| miss(&n.id, "Net Sender has an invalid target IP"))?;
                    OutputSpec::NetSender {
                        node_id: n.id.clone(),
                        target: SocketAddr::new(ip, data.port),
                        channels: data
                            .channels
                            .clamp(1, crate::audio::netaudio::MAX_CHANNELS as u32),
                        codec: data.codec,
                        opus_bitrate: data.opus_bitrate,
                        opus_application: data.opus_application,
                        sample_rate: data.sample_rate.filter(|_| data.codec != NetCodec::Opus),
                    }
                }
                NodeKind::WebRtcCollaborator => {
                    let data: WebRtcCollaboratorData = parse(n.data, "WebRtcCollaborator")?;
                    OutputSpec::WebRtcSend {
                        node_id: n.id.clone(),
                        channels: data
                            .channels
                            .clamp(1, crate::audio::netaudio::MAX_CHANNELS as u32),
                        opus_bitrate: data.opus_bitrate,
                        opus_application: data.opus_application,
                    }
                }
                _ => unreachable!(),
            })
        })();
        match resolved {
            Ok(spec) => result.push(ValidOutput {
                id: n.id.clone(),
                spec,
            }),
            Err(e) if routed.contains(n.id.as_str()) => return Err(e),
            Err(_) => continue,
        }
    }
    Ok(result)
}

fn resolve_effects(nodes: &[RoleNode<'_>], keep: &HashSet<&str>) -> AppResult<Vec<ValidEffect>> {
    let mut result = Vec::new();
    for n in nodes {
        if n.role != NodeCategory::Effect || !keep.contains(n.id.as_str()) {
            continue;
        }
        result.push(ValidEffect {
            id: n.id.clone(),
            spec: effect_from_node(n)?,
        });
    }
    Ok(result)
}

fn bfs_forward<'a>(
    nodes: &'a [RoleNode<'a>],
    outgoing: &HashMap<&'a str, Vec<&'a str>>,
    start_role: NodeCategory,
) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut stack: Vec<&str> = nodes
        .iter()
        .filter(|n| n.role == start_role)
        .map(|n| n.id.as_str())
        .collect();
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur) {
            continue;
        }
        if let Some(kids) = outgoing.get(cur) {
            for &k in kids {
                stack.push(k);
            }
        }
    }
    seen
}

fn bfs_backward_pred<'a>(
    nodes: &'a [RoleNode<'a>],
    incoming: &HashMap<&'a str, Vec<&'a str>>,
    is_terminal: impl Fn(&RoleNode<'_>) -> bool,
) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut stack: Vec<&str> = nodes
        .iter()
        .filter(|n| is_terminal(n))
        .map(|n| n.id.as_str())
        .collect();
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur) {
            continue;
        }
        if let Some(parents) = incoming.get(cur) {
            for &p in parents {
                stack.push(p);
            }
        }
    }
    seen
}

fn effect_from_node(n: &RoleNode<'_>) -> AppResult<EffectSpec> {
    Ok(match n.kind {
        NodeKind::Gain => EffectSpec::Gain(parse(n.data, "Gain")?),
        NodeKind::Mute => EffectSpec::Mute(parse(n.data, "Mute")?),
        NodeKind::ChannelBalance => EffectSpec::ChannelBalance(parse(n.data, "ChannelBalance")?),
        NodeKind::Saturator => EffectSpec::Saturator(parse(n.data, "Saturator")?),
        NodeKind::Eq => EffectSpec::Eq(parse(n.data, "Eq")?),
        NodeKind::LevelMeter => EffectSpec::LevelMeter(parse(n.data, "LevelMeter")?),
        NodeKind::LufsMeter => EffectSpec::LufsMeter(parse(n.data, "LufsMeter")?),
        NodeKind::Waveform => EffectSpec::Waveform(parse(n.data, "Waveform")?),
        NodeKind::Spectrum => EffectSpec::Spectrum(parse(n.data, "Spectrum")?),
        NodeKind::Limiter => EffectSpec::Limiter(parse(n.data, "Limiter")?),
        NodeKind::Compressor => EffectSpec::Compressor(parse(n.data, "Compressor")?),
        NodeKind::NoiseGate => EffectSpec::NoiseGate(parse(n.data, "NoiseGate")?),
        NodeKind::Delay => EffectSpec::Delay(parse(n.data, "Delay")?),
        NodeKind::Reverb => EffectSpec::Reverb(parse(n.data, "Reverb")?),
        NodeKind::NoiseSuppressor => EffectSpec::NoiseSuppressor(parse(n.data, "NoiseSuppressor")?),
        NodeKind::Declick => EffectSpec::Declick(parse(n.data, "Declick")?),
        NodeKind::DeEsser => EffectSpec::DeEsser(parse(n.data, "DeEsser")?),
        NodeKind::Plugin => {
            let data: PluginData = parse(n.data, "Plugin")?;
            EffectSpec::Plugin {
                node_id: n.id.clone(),
                format: data.format,
                path: data.path,
                plugin_id: data.plugin_id,
                bypassed: data.bypassed,
                state: data.state,
            }
        }
        _ => unreachable!("non-effect kind passed to effect_from_node"),
    })
}

fn parse<T: for<'de> Deserialize<'de>>(value: &serde_json::Value, ctx: &str) -> AppResult<T> {
    serde_json::from_value::<T>(value.clone())
        .map_err(|e| AppError::Validation(format!("invalid {ctx} data: {e}")))
}

fn miss(node_id: &str, msg: &str) -> AppError {
    AppError::Validation(format!("{msg} (node {node_id})"))
}

fn choose_file_err(node_id: &str, reason: &str) -> AppError {
    AppError::Validation(format!("choose-file (node {node_id}): {reason}"))
}

fn check_acyclic(nodes: &[RoleNode<'_>], outgoing: &HashMap<&str, Vec<&str>>) -> AppResult<()> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        Unseen,
        InProgress,
        Done,
    }
    let mut marks: HashMap<&str, Mark> = nodes
        .iter()
        .map(|n| (n.id.as_str(), Mark::Unseen))
        .collect();
    for n in nodes {
        if marks[n.id.as_str()] == Mark::Unseen {
            visit(n.id.as_str(), outgoing, &mut marks)?;
        }
    }
    return Ok(());

    fn visit<'a>(
        cur: &'a str,
        outgoing: &HashMap<&str, Vec<&'a str>>,
        marks: &mut HashMap<&'a str, Mark>,
    ) -> AppResult<()> {
        match marks.get(cur).copied().unwrap_or(Mark::Unseen) {
            Mark::Done => return Ok(()),
            Mark::InProgress => {
                return Err(AppError::Validation(format!(
                    "cycle detected at node {cur}"
                )));
            }
            Mark::Unseen => {}
        }
        marks.insert(cur, Mark::InProgress);
        if let Some(kids) = outgoing.get(cur) {
            for &k in kids {
                visit(k, outgoing, marks)?;
            }
        }
        marks.insert(cur, Mark::Done);
        Ok(())
    }
}
