export type NodeCategory = 'input' | 'output' | 'effect';

export type NodeKind =
	| 'microphone'
	| 'systemAudio'
	| 'appAudio'
	| 'speaker'
	| 'fileRecording'
	| 'gain'
	| 'mute'
	| 'channelBalance'
	| 'saturator'
	| 'eq'
	| 'levelMeter'
	| 'lufsMeter'
	| 'limiter';

export interface MicrophoneNodeData extends Record<string, unknown> {
	deviceId: string | null;
}

export interface SystemAudioNodeData extends Record<string, unknown> {
	excludeCurrentApp: boolean;
}

export interface AppAudioNodeData extends Record<string, unknown> {
	bundleId: string | null;
}

export interface SpeakerNodeData extends Record<string, unknown> {
	deviceId: string | null;
}

export interface FileRecordingNodeData extends Record<string, unknown> {
	filePath: string | null;
}

export interface GainNodeData extends Record<string, unknown> {
	gainDb: number;
}

export interface MuteNodeData extends Record<string, unknown> {
	muted: boolean;
}

export interface ChannelBalanceNodeData extends Record<string, unknown> {
	leftGainDb: number;
	rightGainDb: number;
}

export interface SaturatorNodeData extends Record<string, unknown> {
	thresholdDb: number;
	driveDb: number;
}

export interface EqNodeData extends Record<string, unknown> {
	/** Per-band gain in dB at ISO octave centres 32/64/125/250/500/1k/2k/4k/8k/16k. */
	gainsDb: number[];
}

export interface LevelMeterNodeData extends Record<string, unknown> {
	// no params yet — just visualises the live signal
}

export interface LufsMeterNodeData extends Record<string, unknown> {
	/** Compliance target LUFS for the Integrated readout colour, or `null` for free mode. */
	target: number | null;
}

export interface LimiterNodeData extends Record<string, unknown> {
	ceilingDb: number;
	lookaheadMs: number;
	releaseMs: number;
}

export type NodeDataMap = {
	microphone: MicrophoneNodeData;
	systemAudio: SystemAudioNodeData;
	appAudio: AppAudioNodeData;
	speaker: SpeakerNodeData;
	fileRecording: FileRecordingNodeData;
	gain: GainNodeData;
	mute: MuteNodeData;
	channelBalance: ChannelBalanceNodeData;
	saturator: SaturatorNodeData;
	eq: EqNodeData;
	levelMeter: LevelMeterNodeData;
	lufsMeter: LufsMeterNodeData;
	limiter: LimiterNodeData;
};

export type AnyNodeData = NodeDataMap[NodeKind];

export interface PipelineNode<K extends NodeKind = NodeKind> {
	id: string;
	kind: K;
	data: NodeDataMap[K];
	position: { x: number; y: number };
}

export interface PipelineEdge {
	id: string;
	source: string;
	target: string;
}

export interface Pipeline {
	id: string;
	name: string;
	nodes: PipelineNode[];
	edges: PipelineEdge[];
	createdAt: number;
	updatedAt: number;
}
