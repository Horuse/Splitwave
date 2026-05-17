use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use ts_rs::TS;

use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
pub struct GraphSpec {
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
}

#[derive(Debug, Deserialize)]
pub struct NodeSpec {
    pub id: String,
    pub kind: NodeKind,
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeSpec {
    #[allow(dead_code)]
    pub id: String,
    pub source: String,
    pub target: String,
    /// `Some("sidechain")` routes to an effect's sidechain key input.
    pub target_handle: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Hash, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NodeKind {
    Microphone,
    SystemAudio,
    AppAudio,
    Speaker,
    FileRecording,
    Gain,
    Mute,
    ChannelBalance,
    Saturator,
    Eq,
    LevelMeter,
    LufsMeter,
    Waveform,
    Limiter,
    Compressor,
    NoiseGate,
    Delay,
    Reverb,
    AudioFile,
}

impl NodeKind {
    pub fn category(self) -> NodeCategory {
        match self {
            NodeKind::Microphone
            | NodeKind::SystemAudio
            | NodeKind::AppAudio
            | NodeKind::AudioFile => NodeCategory::Input,
            NodeKind::Speaker | NodeKind::FileRecording => NodeCategory::Output,
            NodeKind::Gain
            | NodeKind::Mute
            | NodeKind::ChannelBalance
            | NodeKind::Saturator
            | NodeKind::Eq
            | NodeKind::LevelMeter
            | NodeKind::LufsMeter
            | NodeKind::Waveform
            | NodeKind::Limiter
            | NodeKind::Compressor
            | NodeKind::NoiseGate
            | NodeKind::Delay
            | NodeKind::Reverb => NodeCategory::Effect,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeCategory {
    Input,
    Output,
    Effect,
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MicrophoneData {
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SystemAudioData {
    #[serde(default = "default_true")]
    pub exclude_current_app: bool,
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppAudioData {
    pub bundle_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioFileData {
    pub file_path: Option<String>,
    #[serde(default)]
    pub loop_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SpeakerData {
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum WavBitDepth {
    F32,
    I24,
    I16,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum FlacBitDepth {
    I24,
    I16,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum AiffBitDepth {
    I24,
    I16,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum FlacCompression {
    Fast,
    Default,
    Best,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum OpusApplication {
    Audio,
    Voip,
    LowDelay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[ts(export)]
pub enum RecordingFormat {
    Wav { #[serde(rename = "bitDepth")] bit_depth: WavBitDepth },
    Flac {
        #[serde(rename = "bitDepth")] bit_depth: FlacBitDepth,
        compression: FlacCompression,
    },
    Opus {
        bitrate: u32,
        application: OpusApplication,
    },
    Mp3 {
        #[serde(rename = "bitrateKbps")] bitrate_kbps: u32,
    },
    Aac {
        bitrate: u32,
    },
    Aiff {
        #[serde(rename = "bitDepth")] bit_depth: AiffBitDepth,
    },
}

impl Default for RecordingFormat {
    fn default() -> Self {
        RecordingFormat::Wav {
            bit_depth: WavBitDepth::F32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileRecordingData {
    pub file_path: Option<String>,
    #[serde(default)]
    pub format: RecordingFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GainData {
    pub gain_db: f32,
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MuteData {
    pub muted: bool,
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChannelBalanceData {
    pub left_gain_db: f32,
    pub right_gain_db: f32,
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SaturatorData {
    pub threshold_db: f32,
    pub drive_db: f32,
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EqData {
    /// One gain per ISO octave band (see `EQ_FREQUENCIES_HZ` in effects.rs).
    pub gains_db: [f32; 10],
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct LevelMeterData {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct LufsMeterData {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct WaveformData {}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LimiterData {
    pub ceiling_db: f32,
    pub lookahead_ms: f32,
    pub release_ms: f32,
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CompressorData {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
    pub makeup_db: f32,
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoiseGateData {
    pub threshold_db: f32,
    pub range_db: f32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub release_ms: f32,
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DelayData {
    pub time_ms: f32,
    pub feedback: f32,
    pub mix: f32,
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReverbData {
    pub room_size: f32,
    pub damping: f32,
    pub width: f32,
    pub mix: f32,
    #[serde(default)]
    pub bypassed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSpec {
    Microphone { device_id: String },
    SystemAudio { exclude_current_app: bool },
    AppAudio { bundle_id: String },
    AudioFile { file_path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputSpec {
    Speaker { device_id: String },
    FileRecording { file_path: String, format: RecordingFormat },
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectSpec {
    Gain(GainData),
    Mute(MuteData),
    ChannelBalance(ChannelBalanceData),
    Saturator(SaturatorData),
    Eq(EqData),
    LevelMeter(LevelMeterData),
    LufsMeter(LufsMeterData),
    Waveform(WaveformData),
    Limiter(LimiterData),
    Compressor(CompressorData),
    NoiseGate(NoiseGateData),
    Delay(DelayData),
    Reverb(ReverbData),
}

impl EffectSpec {
    pub fn bypassed(&self) -> bool {
        match self {
            EffectSpec::Gain(d) => d.bypassed,
            EffectSpec::Mute(d) => d.bypassed,
            EffectSpec::ChannelBalance(d) => d.bypassed,
            EffectSpec::Saturator(d) => d.bypassed,
            EffectSpec::Eq(d) => d.bypassed,
            EffectSpec::Limiter(d) => d.bypassed,
            EffectSpec::Compressor(d) => d.bypassed,
            EffectSpec::NoiseGate(d) => d.bypassed,
            EffectSpec::Delay(d) => d.bypassed,
            EffectSpec::Reverb(d) => d.bypassed,
            EffectSpec::LevelMeter(_) | EffectSpec::LufsMeter(_) | EffectSpec::Waveform(_) => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidInput {
    pub id: String,
    pub spec: InputSpec,
}

#[derive(Debug, Clone)]
pub struct ValidOutput {
    pub id: String,
    pub spec: OutputSpec,
}

#[derive(Debug, Clone)]
pub struct ValidEffect {
    pub id: String,
    pub spec: EffectSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Main,
    Sidechain,
}

#[derive(Debug, Clone)]
pub struct ValidEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

/// Validated DAG. Effects may have multiple incoming edges (mixer-bus
/// behaviour), at most one outgoing edge. Inputs may fan out to many
/// downstream nodes. The engine assembles a per-output sub-graph from these
/// fields at start time.
#[derive(Debug, Clone)]
pub struct ValidGraph {
    pub inputs: Vec<ValidInput>,
    pub outputs: Vec<ValidOutput>,
    pub effects: Vec<ValidEffect>,
    pub edges: Vec<ValidEdge>,
}

impl GraphSpec {
    /// Rules:
    /// - Inputs may fan out to many downstream nodes; if none, they're dropped.
    /// - Outputs may receive many incoming edges (mixed at the output).
    /// - Effects may have ≥1 incoming (act as a mixer-bus) and ≤1 outgoing.
    /// - Anything not on a path from some input to some output is dropped.
    /// - Cycles are rejected.
    pub fn validate(&self) -> AppResult<ValidGraph> {
        let nodes_by_id: HashMap<&str, &NodeSpec> =
            self.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

        let mut outgoing: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &self.edges {
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
                if n.kind.category() == NodeCategory::Input {
                    return Err(AppError::Validation(format!(
                        "edge points into input node {:?}",
                        n.id
                    )));
                }
            }
            // Edges out of an output node likewise.
            if let Some(n) = nodes_by_id.get(edge.source.as_str()) {
                if n.kind.category() == NodeCategory::Output {
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

        check_acyclic(&self.nodes, &outgoing)?;

        // Monitor mode: no output but ≥1 analyzer (LevelMeter, LufsMeter) acts as terminal.
        let has_outputs = self.nodes.iter().any(|n| n.kind.category() == NodeCategory::Output);
        let has_analyzers = self
            .nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::LevelMeter | NodeKind::LufsMeter | NodeKind::Waveform));
        if !has_outputs && !has_analyzers {
            return Err(AppError::Validation(
                "no routing — connect an input to an output or a meter".into(),
            ));
        }

        let reachable_from_inputs = bfs_forward(&self.nodes, &outgoing, NodeCategory::Input);
        let reachable_from_terminals: HashSet<&str> = if has_outputs {
            bfs_backward_pred(&self.nodes, &incoming, |n| {
                n.kind.category() == NodeCategory::Output
            })
        } else {
            bfs_backward_pred(&self.nodes, &incoming, |n| {
                matches!(n.kind, NodeKind::LevelMeter | NodeKind::LufsMeter | NodeKind::Waveform)
            })
        };
        let keep: HashSet<&str> = reachable_from_inputs
            .intersection(&reachable_from_terminals)
            .copied()
            .collect();

        let inputs = self.resolve_inputs(&keep)?;
        let outputs = self.resolve_outputs(&keep)?;
        let effects = self.resolve_effects(&keep)?;

        let edges: Vec<ValidEdge> = self
            .edges
            .iter()
            .filter(|e| keep.contains(e.source.as_str()) && keep.contains(e.target.as_str()))
            .map(|e| ValidEdge {
                from: e.source.clone(),
                to: e.target.clone(),
                kind: match e.target_handle.as_deref() {
                    Some("sidechain") => EdgeKind::Sidechain,
                    _ => EdgeKind::Main,
                },
            })
            .collect();

        Ok(ValidGraph {
            inputs,
            outputs,
            effects,
            edges,
        })
    }

    fn resolve_inputs(&self, keep: &HashSet<&str>) -> AppResult<Vec<ValidInput>> {
        let mut result = Vec::new();
        for n in &self.nodes {
            if n.kind.category() != NodeCategory::Input || !keep.contains(n.id.as_str()) {
                continue;
            }
            let spec = match n.kind {
                NodeKind::Microphone => {
                    let data: MicrophoneData = parse(&n.data, "Microphone")?;
                    InputSpec::Microphone {
                        device_id: data
                            .device_id
                            .ok_or_else(|| miss(&n.id, "Microphone has no device selected"))?,
                    }
                }
                NodeKind::SystemAudio => {
                    let data: SystemAudioData = parse(&n.data, "SystemAudio")?;
                    InputSpec::SystemAudio {
                        exclude_current_app: data.exclude_current_app,
                    }
                }
                NodeKind::AppAudio => {
                    let data: AppAudioData = parse(&n.data, "AppAudio")?;
                    InputSpec::AppAudio {
                        bundle_id: data
                            .bundle_id
                            .ok_or_else(|| miss(&n.id, "App Audio has no application selected"))?,
                    }
                }
                NodeKind::AudioFile => {
                    let data: AudioFileData = parse(&n.data, "AudioFile")?;
                    InputSpec::AudioFile {
                        file_path: data
                            .file_path
                            .ok_or_else(|| miss(&n.id, "Audio File has no file selected"))?,
                    }
                }
                _ => unreachable!(),
            };
            result.push(ValidInput {
                id: n.id.clone(),
                spec,
            });
        }
        Ok(result)
    }

    fn resolve_outputs(&self, keep: &HashSet<&str>) -> AppResult<Vec<ValidOutput>> {
        let mut result = Vec::new();
        for n in &self.nodes {
            if n.kind.category() != NodeCategory::Output || !keep.contains(n.id.as_str()) {
                continue;
            }
            let spec = match n.kind {
                NodeKind::Speaker => {
                    let data: SpeakerData = parse(&n.data, "Speaker")?;
                    OutputSpec::Speaker {
                        device_id: data
                            .device_id
                            .ok_or_else(|| miss(&n.id, "Speaker has no device selected"))?,
                    }
                }
                NodeKind::FileRecording => {
                    let data: FileRecordingData = parse(&n.data, "FileRecording")?;
                    OutputSpec::FileRecording {
                        file_path: data
                            .file_path
                            .ok_or_else(|| miss(&n.id, "File Recording has no path"))?,
                        format: data.format,
                    }
                }
                _ => unreachable!(),
            };
            result.push(ValidOutput {
                id: n.id.clone(),
                spec,
            });
        }
        Ok(result)
    }

    fn resolve_effects(&self, keep: &HashSet<&str>) -> AppResult<Vec<ValidEffect>> {
        let mut result = Vec::new();
        for n in &self.nodes {
            if n.kind.category() != NodeCategory::Effect || !keep.contains(n.id.as_str()) {
                continue;
            }
            result.push(ValidEffect {
                id: n.id.clone(),
                spec: effect_from_node(n)?,
            });
        }
        Ok(result)
    }
}

fn bfs_forward<'a>(
    nodes: &'a [NodeSpec],
    outgoing: &HashMap<&'a str, Vec<&'a str>>,
    start_category: NodeCategory,
) -> HashSet<&'a str> {
    let mut seen = HashSet::new();
    let mut stack: Vec<&str> = nodes
        .iter()
        .filter(|n| n.kind.category() == start_category)
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
    nodes: &'a [NodeSpec],
    incoming: &HashMap<&'a str, Vec<&'a str>>,
    is_terminal: impl Fn(&NodeSpec) -> bool,
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

fn effect_from_node(n: &NodeSpec) -> AppResult<EffectSpec> {
    Ok(match n.kind {
        NodeKind::Gain => EffectSpec::Gain(parse(&n.data, "Gain")?),
        NodeKind::Mute => EffectSpec::Mute(parse(&n.data, "Mute")?),
        NodeKind::ChannelBalance => EffectSpec::ChannelBalance(parse(&n.data, "ChannelBalance")?),
        NodeKind::Saturator => EffectSpec::Saturator(parse(&n.data, "Saturator")?),
        NodeKind::Eq => EffectSpec::Eq(parse(&n.data, "Eq")?),
        NodeKind::LevelMeter => EffectSpec::LevelMeter(parse(&n.data, "LevelMeter")?),
        NodeKind::LufsMeter => EffectSpec::LufsMeter(parse(&n.data, "LufsMeter")?),
        NodeKind::Waveform => EffectSpec::Waveform(parse(&n.data, "Waveform")?),
        NodeKind::Limiter => EffectSpec::Limiter(parse(&n.data, "Limiter")?),
        NodeKind::Compressor => EffectSpec::Compressor(parse(&n.data, "Compressor")?),
        NodeKind::NoiseGate => EffectSpec::NoiseGate(parse(&n.data, "NoiseGate")?),
        NodeKind::Delay => EffectSpec::Delay(parse(&n.data, "Delay")?),
        NodeKind::Reverb => EffectSpec::Reverb(parse(&n.data, "Reverb")?),
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

fn check_acyclic(nodes: &[NodeSpec], outgoing: &HashMap<&str, Vec<&str>>) -> AppResult<()> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        Unseen,
        InProgress,
        Done,
    }
    let mut marks: HashMap<&str, Mark> = nodes.iter().map(|n| (n.id.as_str(), Mark::Unseen)).collect();
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
                return Err(AppError::Validation(format!("cycle detected at node {cur}")));
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
