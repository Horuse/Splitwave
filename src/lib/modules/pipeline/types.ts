import type { AppAudioData } from './generated/AppAudioData';
import type { AudioFileData } from './generated/AudioFileData';
import type { ChannelBalanceData } from './generated/ChannelBalanceData';
import type { CompressorData } from './generated/CompressorData';
import type { DelayData } from './generated/DelayData';
import type { EqData } from './generated/EqData';
import type { FileRecordingData } from './generated/FileRecordingData';
import type { GainData } from './generated/GainData';
import type { LevelMeterData } from './generated/LevelMeterData';
import type { LimiterData } from './generated/LimiterData';
import type { LufsMeterData } from './generated/LufsMeterData';
import type { WaveformData } from './generated/WaveformData';
import type { SpectrumData } from './generated/SpectrumData';
import type { DeclickData } from './generated/DeclickData';
import type { DeEsserData } from './generated/DeEsserData';
import type { MicrophoneData } from './generated/MicrophoneData';
import type { MicrophoneArrayData } from './generated/MicrophoneArrayData';
import type { NetReceiverData } from './generated/NetReceiverData';
import type { NetSenderData } from './generated/NetSenderData';
import type { MuteData } from './generated/MuteData';
import type { NoiseGateData } from './generated/NoiseGateData';
import type { NoiseSuppressorData } from './generated/NoiseSuppressorData';
import type { PluginData } from './generated/PluginData';
import type { ReverbData } from './generated/ReverbData';
import type { SaturatorData } from './generated/SaturatorData';
import type { SpeakerData } from './generated/SpeakerData';
import type { SystemAudioData } from './generated/SystemAudioData';
import type { WebRtcCollaboratorData } from './generated/WebRtcCollaboratorData';

export type { AiffBitDepth } from './generated/AiffBitDepth';
export type { FlacBitDepth } from './generated/FlacBitDepth';
export type { FlacCompression } from './generated/FlacCompression';
export type { NetCodec } from './generated/NetCodec';
export type { MicrophoneArrayAlgorithm } from './generated/MicrophoneArrayAlgorithm';
export type { MicrophoneArrayCalibration } from './generated/MicrophoneArrayCalibration';
export type { MicrophoneArrayCalibrationState } from './generated/MicrophoneArrayCalibrationState';
export type { MicrophoneArrayChannelQuality } from './generated/MicrophoneArrayChannelQuality';
export type { MicrophoneArrayGeometry } from './generated/MicrophoneArrayGeometry';
export type { MicrophoneArrayMember } from './generated/MicrophoneArrayMember';
export type { MicrophoneArrayPoint } from './generated/MicrophoneArrayPoint';
export type { MicrophoneArraySource } from './generated/MicrophoneArraySource';
export type { MicrophoneArrayTarget } from './generated/MicrophoneArrayTarget';
export type { NodeKind } from './generated/NodeKind';
export type { OpusApplication } from './generated/OpusApplication';
export type { RecordingFormat } from './generated/RecordingFormat';
export type { WavBitDepth } from './generated/WavBitDepth';

import type { NodeKind } from './generated/NodeKind';

export type NodeCategory = 'input' | 'output' | 'monitor' | 'network' | 'effect';

// xyflow requires node data to satisfy `Record<string, unknown>`; intersecting
// gives generated types that constraint without us redeclaring fields.
type XyData<T> = T & Record<string, unknown>;

export type MicrophoneNodeData = XyData<MicrophoneData>;
export type MicrophoneArrayNodeData = XyData<MicrophoneArrayData>;
export type SystemAudioNodeData = XyData<SystemAudioData>;
export type AppAudioNodeData = XyData<AppAudioData>;
export type AudioFileNodeData = XyData<AudioFileData>;
export type SpeakerNodeData = XyData<SpeakerData>;
export type FileRecordingNodeData = XyData<FileRecordingData>;
export type GainNodeData = XyData<GainData>;
// FE-only, invisible to the engine: `hotkey` is a Tauri accelerator that toggles
// `muted`, the `cue*` pair configures the spoken confirmation on toggle.
// `pushToTalk` swaps the hotkey from a toggle to hold-to-unmute.
export type MuteNodeData = XyData<
	MuteData & {
		hotkey?: string;
		cueEnabled?: boolean;
		cueDeviceId?: string;
		cueVolume?: number;
		pushToTalk?: boolean;
	}
>;
export type ChannelBalanceNodeData = XyData<ChannelBalanceData>;
export type SaturatorNodeData = XyData<SaturatorData>;
export type EqNodeData = XyData<EqData>;
export type LevelMeterNodeData = XyData<LevelMeterData>;
export type LimiterNodeData = XyData<LimiterData>;
export type WaveformNodeData = XyData<WaveformData & { segs?: number }>;
// `smoothing` (0..1) is a FE-only display ballistic — how slowly bars rise and
// fall — with no meaning to the engine.
export type SpectrumNodeData = XyData<SpectrumData & { smoothing?: number }>;
export type CompressorNodeData = XyData<CompressorData>;
export type NoiseGateNodeData = XyData<NoiseGateData>;
export type DelayNodeData = XyData<DelayData>;
export type ReverbNodeData = XyData<ReverbData>;
export type NoiseSuppressorNodeData = XyData<NoiseSuppressorData>;
export type DeclickNodeData = XyData<DeclickData>;
export type DeEsserNodeData = XyData<DeEsserData>;
// `name` / `vendor` are FE-only display labels for the chosen plugin; the
// engine reads path / pluginId / bypassed / state from PluginData.
export type PluginNodeData = XyData<PluginData & { name?: string; vendor?: string; showParams?: boolean }>;
// `name` is a FE-only participant label shared over the ctrl channel; the
// engine ignores it, so it lives outside the Rust WebRtcCollaboratorData struct.
export type WebRtcCollaboratorNodeData = XyData<WebRtcCollaboratorData & { name?: string }>;
export type NetReceiverNodeData = XyData<NetReceiverData>;
export type NetSenderNodeData = XyData<NetSenderData>;

// Compliance target is a FE-only UI hint (colours the Integrated readout) — the
// engine has no use for it, so it lives outside the Rust LufsMeterData struct.
export type LufsMeterNodeData = XyData<LufsMeterData & { target: number | null; profile?: string }>;

export type NodeDataMap = {
	microphone: MicrophoneNodeData;
	microphoneArray: MicrophoneArrayNodeData;
	systemAudio: SystemAudioNodeData;
	appAudio: AppAudioNodeData;
	audioFile: AudioFileNodeData;
	speaker: SpeakerNodeData;
	fileRecording: FileRecordingNodeData;
	gain: GainNodeData;
	mute: MuteNodeData;
	channelBalance: ChannelBalanceNodeData;
	saturator: SaturatorNodeData;
	eq: EqNodeData;
	levelMeter: LevelMeterNodeData;
	lufsMeter: LufsMeterNodeData;
	waveform: WaveformNodeData;
	spectrum: SpectrumNodeData;
	limiter: LimiterNodeData;
	compressor: CompressorNodeData;
	noiseGate: NoiseGateNodeData;
	delay: DelayNodeData;
	reverb: ReverbNodeData;
	noiseSuppressor: NoiseSuppressorNodeData;
	declick: DeclickNodeData;
	deEsser: DeEsserNodeData;
	plugin: PluginNodeData;
	webRtcCollaborator: WebRtcCollaboratorNodeData;
	netReceiver: NetReceiverNodeData;
	netSender: NetSenderNodeData;
};

export type AnyNodeData = NodeDataMap[NodeKind];

export interface PipelineNode<K extends NodeKind = NodeKind> {
	id: string;
	kind: K;
	data: NodeDataMap[K];
	position: { x: number; y: number };
	width?: number;
	height?: number;
}

export interface PipelineEdge {
	id: string;
	source: string;
	sourceHandle?: string;
	target: string;
	targetHandle?: string;
}

export interface Pipeline {
	id: string;
	name: string;
	nodes: PipelineNode[];
	edges: PipelineEdge[];
	createdAt: number;
	updatedAt: number;
	/** Schema version; absent means pre-versioning, see `pipeline/migrations`. */
	version?: number;
	/** Immutable catalog origin; later template edits never rewrite this pipeline. */
	sourceTemplateId?: string;
	sourceTemplateVersion?: number;
}
