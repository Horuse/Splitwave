use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};

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
    /// `Some("peer:<id>")` selects a WebRTC per-peer output; `None` is the main out.
    pub source_handle: Option<String>,
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
    Spectrum,
    Limiter,
    Compressor,
    NoiseGate,
    Delay,
    Reverb,
    NoiseSuppressor,
    Declick,
    DeEsser,
    AudioFile,
    WebRtcCollaborator,
    NetReceiver,
    NetSender,
    Plugin,
}

impl NodeKind {
    pub fn category(self) -> NodeCategory {
        match self {
            NodeKind::Microphone
            | NodeKind::SystemAudio
            | NodeKind::AppAudio
            | NodeKind::NetReceiver
            | NodeKind::AudioFile => NodeCategory::Input,
            NodeKind::Speaker | NodeKind::FileRecording | NodeKind::NetSender => {
                NodeCategory::Output
            }
            NodeKind::Gain
            | NodeKind::Mute
            | NodeKind::ChannelBalance
            | NodeKind::Saturator
            | NodeKind::Eq
            | NodeKind::LevelMeter
            | NodeKind::LufsMeter
            | NodeKind::Waveform
            | NodeKind::Spectrum
            | NodeKind::Limiter
            | NodeKind::Compressor
            | NodeKind::NoiseGate
            | NodeKind::Delay
            | NodeKind::Reverb
            | NodeKind::NoiseSuppressor
            | NodeKind::Declick
            | NodeKind::DeEsser
            | NodeKind::Plugin => NodeCategory::Effect,
            // Two destinations in one UI node: it sends to peers and emits what
            // they send back. `expand_roles` splits it into an output half and
            // an input half, so no single category is ever asked for.
            NodeKind::WebRtcCollaborator => {
                unreachable!("WebRtcCollaborator is split by expand_roles")
            }
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
    #[serde(default = "default_one")]
    pub volume: f32,
}
fn default_true() -> bool {
    true
}
fn default_one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppAudioData {
    pub bundle_id: Option<String>,
    #[serde(default = "default_one")]
    pub volume: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioFileData {
    pub file_path: Option<String>,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default = "default_one")]
    pub volume: f32,
    #[serde(default = "default_true")]
    pub auto_start: bool,
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
    Wav {
        #[serde(rename = "bitDepth")]
        bit_depth: WavBitDepth,
    },
    Flac {
        #[serde(rename = "bitDepth")]
        bit_depth: FlacBitDepth,
        compression: FlacCompression,
    },
    Opus {
        bitrate: u32,
        application: OpusApplication,
    },
    Mp3 {
        #[serde(rename = "bitrateKbps")]
        bitrate_kbps: u32,
    },
    Aac {
        bitrate: u32,
    },
    Aiff {
        #[serde(rename = "bitDepth")]
        bit_depth: AiffBitDepth,
    },
}

impl Default for RecordingFormat {
    fn default() -> Self {
        RecordingFormat::Wav {
            bit_depth: WavBitDepth::F32,
        }
    }
}

impl RecordingFormat {
    /// LAME, the plain Opus encoder and Apple's AAC encoder are two-channel
    /// (probed: CoreAudio's AAC rejects 3+ channels); FLAC caps by spec.
    pub fn max_channels(self) -> u16 {
        match self {
            RecordingFormat::Mp3 { .. }
            | RecordingFormat::Opus { .. }
            | RecordingFormat::Aac { .. } => 2,
            RecordingFormat::Flac { .. } => 8,
            RecordingFormat::Wav { .. } | RecordingFormat::Aiff { .. } => 512,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RecordingMode {
    New,
    Overwrite,
    Append,
}

impl Default for RecordingMode {
    fn default() -> Self {
        RecordingMode::New
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileRecordingData {
    pub file_path: Option<String>,
    #[serde(default)]
    pub format: RecordingFormat,
    #[serde(default)]
    pub mode: RecordingMode,
    #[serde(default = "default_two")]
    pub channels: u16,
    /// Pinned file sample rate; defaults to 48 kHz so the recorded rate is
    /// always explicit. Ignored for Opus/Mp3, which are locked to 48 kHz.
    #[serde(default = "default_rec_sample_rate")]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub waveform_hidden: bool,
}

fn default_rec_sample_rate() -> Option<u32> {
    Some(48_000)
}

fn default_two() -> u16 {
    2
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
pub struct DeclickData {
    /// 0..1; higher flags smaller spikes as clicks.
    pub sensitivity: f32,
    /// Longest click span repaired, in milliseconds.
    #[serde(default = "default_declick_width")]
    pub max_width_ms: f32,
    #[serde(default)]
    pub bypassed: bool,
}

fn default_declick_width() -> f32 {
    2.0
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeEsserData {
    /// Crossover / detector frequency in Hz; the band above it is de-essed.
    pub frequency: f32,
    /// Level (dBFS) above which the sibilant band is compressed.
    pub threshold_db: f32,
    /// Compression ratio applied to the sibilant band.
    pub ratio: f32,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SpectrumData {}

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

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NoiseSuppressorData {
    pub attenuation_limit_db: f32,
    // Runtime knobs mirroring the upstream DeepFilterNet LADSPA plugin.
    #[serde(default)]
    pub post_filter_beta: f32,
    #[serde(default = "default_min_thresh_db")]
    pub min_thresh_db: f32,
    #[serde(default = "default_max_erb_thresh_db")]
    pub max_erb_thresh_db: f32,
    #[serde(default = "default_max_df_thresh_db")]
    pub max_df_thresh_db: f32,
    #[serde(default)]
    pub bypassed: bool,
}
fn default_min_thresh_db() -> f32 {
    -10.0
}
fn default_max_erb_thresh_db() -> f32 {
    30.0
}
fn default_max_df_thresh_db() -> f32 {
    20.0
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetReceiverData {
    pub port: u16,
    #[serde(default = "default_channels")]
    pub channels: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum NetCodec {
    PcmF32,
    PcmI16,
    Opus,
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetSenderData {
    pub target_ip: String,
    pub port: u16,
    #[serde(default = "default_channels")]
    pub channels: u32,
    pub codec: NetCodec,
    pub opus_bitrate: u32,
    pub opus_application: OpusApplication,
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginData {
    /// None until a plugin is picked, which pairs with an empty `path`.
    #[serde(default)]
    pub format: Option<crate::audio::plugins::PluginFormat>,
    pub path: String,
    pub plugin_id: String,
    #[serde(default)]
    pub bypassed: bool,
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WebRtcCollaboratorData {
    pub opus_bitrate: u32,
    pub opus_application: OpusApplication,
    #[serde(default = "default_channels")]
    pub channels: u32,
    #[serde(default = "default_codec")]
    pub codec: NetCodec,
}
fn default_channels() -> u32 {
    1
}
fn default_codec() -> NetCodec {
    NetCodec::Opus
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSpec {
    Microphone {
        device_id: String,
    },
    SystemAudio {
        exclude_current_app: bool,
    },
    AppAudio {
        bundle_id: String,
    },
    AudioFile {
        file_path: String,
    },
    NetReceiver {
        port: u16,
    },
    /// Receive half of a WebRTC collaborator: audio arriving from peers, tapped
    /// per peer and per channel out of the session's jitter buffer.
    WebRtcRecv {
        node_id: String,
        opus_bitrate: u32,
        opus_application: OpusApplication,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputSpec {
    Speaker {
        device_id: String,
    },
    FileRecording {
        file_path: String,
        format: RecordingFormat,
        channels: u16,
        mode: RecordingMode,
        sample_rate: Option<u32>,
    },
    NetSender {
        node_id: String,
        target: SocketAddr,
        channels: u32,
        codec: NetCodec,
        opus_bitrate: u32,
        opus_application: OpusApplication,
    },
    /// Send half of a WebRTC collaborator: per-channel audio handed to the
    /// session's encode task. The wire codec is set by the UI, not the graph.
    WebRtcSend {
        node_id: String,
        channels: u32,
        opus_bitrate: u32,
        opus_application: OpusApplication,
    },
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
    Spectrum(SpectrumData),
    Limiter(LimiterData),
    Compressor(CompressorData),
    NoiseGate(NoiseGateData),
    Delay(DelayData),
    Reverb(ReverbData),
    NoiseSuppressor(NoiseSuppressorData),
    Declick(DeclickData),
    DeEsser(DeEsserData),
    Plugin {
        node_id: String,
        format: Option<crate::audio::plugins::PluginFormat>,
        path: String,
        plugin_id: String,
        bypassed: bool,
        // Base64 CLAP state blob restored on instantiation; None keeps defaults.
        state: Option<String>,
    },
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
            EffectSpec::NoiseSuppressor(d) => d.bypassed,
            EffectSpec::Declick(d) => d.bypassed,
            EffectSpec::DeEsser(d) => d.bypassed,
            EffectSpec::Plugin { bypassed, .. } => *bypassed,
            EffectSpec::LevelMeter(_)
            | EffectSpec::LufsMeter(_)
            | EffectSpec::Waveform(_)
            | EffectSpec::Spectrum(_) => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValidInput {
    pub id: String,
    pub spec: InputSpec,
    pub volume: f32,
    pub auto_start: bool,
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
    pub source_handle: Option<String>,
    pub to: String,
    /// Target-side handle, e.g. `Some("ch1")` for a WebRTC bridge channel input.
    pub target_handle: Option<String>,
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
const RECV_SUFFIX: &str = "#recv";

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
        // Keep unrouted input nodes too, so their capture + level meter run
        // before they're wired anywhere; unresolvable ones drop in resolve_inputs.
        let mut keep = routed.clone();
        for n in &nodes {
            if n.role == NodeCategory::Input {
                keep.insert(n.id.as_str());
            }
        }

        let inputs = resolve_inputs(&nodes, &keep, &routed)?;
        let outputs = resolve_outputs(&nodes, &keep)?;
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

        Ok(ValidGraph {
            inputs,
            outputs,
            effects,
            edges,
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

fn resolve_outputs(nodes: &[RoleNode<'_>], keep: &HashSet<&str>) -> AppResult<Vec<ValidOutput>> {
    let mut result = Vec::new();
    for n in nodes {
        if n.role != NodeCategory::Output || !keep.contains(n.id.as_str()) {
            continue;
        }
        let spec = match n.kind {
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
                    // FLAC's format tops out at 655350 Hz (20-bit rate field);
                    // every other recording format caps at 384000.
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
                }
            }
            // Send half of a collaborator: audio wired in goes to peers,
            // which is a destination like any other sender.
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
        };
        result.push(ValidOutput {
            id: n.id.clone(),
            spec,
        });
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

#[cfg(test)]
mod tests {
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
            nodes: vec![mic("m"), collab("w")],
            edges: vec![edge("e", "m", None, "w", Some("ch1"))],
        };
        let v = g.validate().expect("send-only graph is valid");
        // The send half is a destination in its own right: no speaker, no meter,
        // and no monitor needed for the mic to reach peers.
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
            nodes: vec![collab("w"), speaker("s")],
            edges: vec![edge("e", "w", Some("peer:p:0"), "s", None)],
        };
        let v = g.validate().expect("recv-only graph is valid");
        assert_eq!(v.inputs.len(), 1);
        assert_eq!(v.inputs[0].id, "w#recv");
        // The session is keyed by the UI node, so the split suffix is local.
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
            nodes: vec![collab("w")],
            edges: vec![],
        };
        let v = g
            .validate()
            .expect("an unwired collaborator is a destination in waiting");
        assert!(v.inputs.is_empty());
        assert!(v.outputs.is_empty());
    }
}
