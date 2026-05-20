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
import type { MicrophoneData } from './generated/MicrophoneData';
import type { MuteData } from './generated/MuteData';
import type { NoiseGateData } from './generated/NoiseGateData';
import type { ReverbData } from './generated/ReverbData';
import type { SaturatorData } from './generated/SaturatorData';
import type { SpeakerData } from './generated/SpeakerData';
import type { SystemAudioData } from './generated/SystemAudioData';
import type { WebRtcCollaboratorData } from './generated/WebRtcCollaboratorData';

export type { AiffBitDepth } from './generated/AiffBitDepth';
export type { FlacBitDepth } from './generated/FlacBitDepth';
export type { FlacCompression } from './generated/FlacCompression';
export type { NodeKind } from './generated/NodeKind';
export type { OpusApplication } from './generated/OpusApplication';
export type { RecordingFormat } from './generated/RecordingFormat';
export type { WavBitDepth } from './generated/WavBitDepth';

import type { NodeKind } from './generated/NodeKind';

export type NodeCategory = 'input' | 'output' | 'effect' | 'monitor';

// xyflow requires node data to satisfy `Record<string, unknown>`; intersecting
// gives generated types that constraint without us redeclaring fields.
type XyData<T> = T & Record<string, unknown>;

export type MicrophoneNodeData = XyData<MicrophoneData>;
export type SystemAudioNodeData = XyData<SystemAudioData>;
export type AppAudioNodeData = XyData<AppAudioData>;
export type AudioFileNodeData = XyData<AudioFileData>;
export type SpeakerNodeData = XyData<SpeakerData>;
export type FileRecordingNodeData = XyData<FileRecordingData>;
export type GainNodeData = XyData<GainData>;
export type MuteNodeData = XyData<MuteData>;
export type ChannelBalanceNodeData = XyData<ChannelBalanceData>;
export type SaturatorNodeData = XyData<SaturatorData>;
export type EqNodeData = XyData<EqData>;
export type LevelMeterNodeData = XyData<LevelMeterData>;
export type LimiterNodeData = XyData<LimiterData>;
export type WaveformNodeData = XyData<WaveformData & { segs?: number }>;
export type CompressorNodeData = XyData<CompressorData>;
export type NoiseGateNodeData = XyData<NoiseGateData>;
export type DelayNodeData = XyData<DelayData>;
export type ReverbNodeData = XyData<ReverbData>;
export type WebRtcCollaboratorNodeData = XyData<WebRtcCollaboratorData>;

// Compliance target is a FE-only UI hint (colours the Integrated readout) — the
// engine has no use for it, so it lives outside the Rust LufsMeterData struct.
export type LufsMeterNodeData = XyData<LufsMeterData & { target: number | null }>;

export type NodeDataMap = {
	microphone: MicrophoneNodeData;
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
	limiter: LimiterNodeData;
	compressor: CompressorNodeData;
	noiseGate: NoiseGateNodeData;
	delay: DelayNodeData;
	reverb: ReverbNodeData;
	webRtcCollaborator: WebRtcCollaboratorNodeData;
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
}
