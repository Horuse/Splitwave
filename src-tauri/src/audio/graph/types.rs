use std::net::SocketAddr;

use serde::Deserialize;
use ts_rs::TS;

#[derive(Debug, Deserialize)]
pub struct GraphSpec {
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
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
    #[serde(default)]
    pub sample_rate: Option<u32>,
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
        sample_rate: Option<u32>,
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
    pub sample_rate: u32,
}
