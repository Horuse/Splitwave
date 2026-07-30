import type { NodeDataMap, NodeKind } from './types';

/** Lives here rather than in the flow registry so migrations can reach it
 * without importing Svelte components. */
export const DEFAULT_NODE_DATA: { [K in NodeKind]: NodeDataMap[K] } = {
	microphone: { deviceId: null },
	systemAudio: { excludeCurrentApp: true, volume: 1 },
	appAudio: { bundleId: null, volume: 1 },
	audioFile: { filePath: null, loopEnabled: false, volume: 1, autoStart: true },
	speaker: { deviceId: null },
	fileRecording: {
		filePath: null,
		format: { kind: 'wav', bitDepth: 'f32' },
		allowOverwrite: false,
		channels: 2
	},
	gain: { gainDb: 0, bypassed: false },
	mute: { muted: false, bypassed: false },
	channelBalance: { leftGainDb: 0, rightGainDb: 0, bypassed: false },
	saturator: { thresholdDb: -0.3, driveDb: 0, bypassed: false },
	eq: { gainsDb: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], bypassed: false },
	levelMeter: {},
	lufsMeter: { target: -14 },
	waveform: { segs: 4 },
	spectrum: { smoothing: 0.5 },
	limiter: { ceilingDb: -0.3, lookaheadMs: 5, releaseMs: 50, bypassed: false },
	compressor: {
		thresholdDb: -18,
		ratio: 3,
		attackMs: 10,
		releaseMs: 100,
		kneeDb: 6,
		makeupDb: 0,
		bypassed: false
	},
	noiseGate: {
		thresholdDb: -40,
		rangeDb: -40,
		attackMs: 1,
		holdMs: 50,
		releaseMs: 200,
		bypassed: false
	},
	delay: { timeMs: 250, feedback: 0.4, mix: 0.35, bypassed: false },
	reverb: { roomSize: 0.5, damping: 0.5, width: 1, mix: 0.33, bypassed: false },
	noiseSuppressor: {
		attenuationLimitDb: 100,
		postFilterBeta: 0,
		minThreshDb: -10,
		maxErbThreshDb: 30,
		maxDfThreshDb: 20,
		bypassed: false
	},
	declick: { sensitivity: 0.5, maxWidthMs: 2, bypassed: false },
	deEsser: { frequency: 6500, thresholdDb: -30, ratio: 4, bypassed: false },
	plugin: { format: null, path: '', pluginId: '', bypassed: false, state: null },
	webRtcCollaborator: {
		opusBitrate: 96000,
		opusApplication: 'voip',
		channels: 1,
		name: '',
		codec: 'opus'
	},
	netReceiver: { port: 5004, channels: 1 },
	netSender: {
		targetIp: '',
		port: 5004,
		channels: 1,
		codec: 'opus',
		opusBitrate: 96000,
		opusApplication: 'audio'
	}
};

/** Stored data wins, defaults fill gaps: adding a parameter to an effect stays a
 * non-breaking change, since older records simply inherit its default. */
export function withDefaults<K extends NodeKind>(kind: K, stored: unknown): NodeDataMap[K] {
	const base = DEFAULT_NODE_DATA[kind];
	if (!base || typeof stored !== 'object' || stored === null) return base;
	return { ...base, ...(stored as object) } as NodeDataMap[K];
}
