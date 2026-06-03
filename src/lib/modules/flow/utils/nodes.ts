import type { Component } from 'svelte';
import type { NodeTypes } from '@xyflow/svelte';
import type {
	AnyNodeData,
	NodeCategory,
	NodeDataMap,
	NodeKind
} from '$lib/modules/pipeline/types';
import Microphone from '../ui/input/microphone.svelte';
import SystemAudio from '../ui/input/system_audio.svelte';
import AppAudio from '../ui/input/app_audio.svelte';
import AudioFile from '../ui/input/audio_file.svelte';
import Speaker from '../ui/output/speaker.svelte';
import FileRecording from '../ui/output/file_recording.svelte';
import Gain from '../ui/effect/gain.svelte';
import Mute from '../ui/effect/mute.svelte';
import ChannelBalance from '../ui/effect/channel_balance.svelte';
import Saturator from '../ui/effect/saturator.svelte';
import Eq from '../ui/effect/eq.svelte';
import LevelMeter from '../ui/effect/level_meter.svelte';
import LufsMeter from '../ui/effect/lufs_meter.svelte';
import Waveform from '../ui/effect/waveform.svelte';
import Limiter from '../ui/effect/limiter.svelte';
import Compressor from '../ui/effect/compressor.svelte';
import NoiseGate from '../ui/effect/noise_gate.svelte';
import Delay from '../ui/effect/delay.svelte';
import Reverb from '../ui/effect/reverb.svelte';
import NoiseSuppressor from '../ui/effect/noise_suppressor.svelte';

// MIME type used during drag-and-drop from the sidebar.
export const DND_MIME = 'application/x-splitwave-nodekind';

export const PREVIEW_CTX = Symbol('flow-preview');

export interface NodeRegistryEntry<K extends NodeKind = NodeKind> {
	kind: K;
	category: NodeCategory;
	label: string;
	description: string;
	component: Component<any>;
	defaultData: NodeDataMap[K];
}

function entry<K extends NodeKind>(e: NodeRegistryEntry<K>): NodeRegistryEntry {
	return e as NodeRegistryEntry;
}

export const registry: Record<NodeKind, NodeRegistryEntry> = {
	microphone: entry<'microphone'>({
		kind: 'microphone',
		category: 'input',
		label: 'Microphone',
		description: 'Capture from a physical input (built-in mic, USB, audio interface).',
		component: Microphone,
		defaultData: { deviceId: null }
	}),
	systemAudio: entry<'systemAudio'>({
		kind: 'systemAudio',
		category: 'input',
		label: 'System Audio',
		description: 'Capture everything the system is playing.',
		component: SystemAudio,
		defaultData: { excludeCurrentApp: true, volume: 1 }
	}),
	appAudio: entry<'appAudio'>({
		kind: 'appAudio',
		category: 'input',
		label: 'App Audio',
		description: 'Capture audio from a single running application.',
		component: AppAudio,
		defaultData: { bundleId: null, volume: 1 }
	}),
	audioFile: entry<'audioFile'>({
		kind: 'audioFile',
		category: 'input',
		label: 'Audio File',
		description: 'Play a WAV file as a source. With no live inputs the pipeline runs faster than real time.',
		component: AudioFile,
		defaultData: { filePath: null, loopEnabled: false, volume: 1, autoStart: true }
	}),
	speaker: entry<'speaker'>({
		kind: 'speaker',
		category: 'output',
		label: 'Speaker',
		description: 'Route audio to a physical output (built-in speakers, headphones, interface).',
		component: Speaker,
		defaultData: { deviceId: null }
	}),
	fileRecording: entry<'fileRecording'>({
		kind: 'fileRecording',
		category: 'output',
		label: 'File Recording',
		description: 'Record to WAV / FLAC / AIFF (lossless), or Opus / MP3 / AAC (lossy).',
		component: FileRecording,
		defaultData: {
			filePath: null,
			format: { kind: 'wav', bitDepth: 'f32' },
			allowOverwrite: false
		}
	}),
	gain: entry<'gain'>({
		kind: 'gain',
		category: 'effect',
		label: 'Gain',
		description: 'Linear amplitude scaling in dB.',
		component: Gain,
		defaultData: { gainDb: 0, bypassed: false }
	}),
	mute: entry<'mute'>({
		kind: 'mute',
		category: 'effect',
		label: 'Mute',
		description: 'Silence the signal.',
		component: Mute,
		defaultData: { muted: false, bypassed: false }
	}),
	channelBalance: entry<'channelBalance'>({
		kind: 'channelBalance',
		category: 'effect',
		label: 'Channel Balance',
		description: 'Separate gain for left and right channels.',
		component: ChannelBalance,
		defaultData: { leftGainDb: 0, rightGainDb: 0, bypassed: false }
	}),
	saturator: entry<'saturator'>({
		kind: 'saturator',
		category: 'effect',
		label: 'Saturator',
		description: 'Soft tanh saturator — smooth distortion, no hard clipping. Not a brick-wall limiter.',
		component: Saturator,
		defaultData: { thresholdDb: -0.3, driveDb: 0, bypassed: false }
	}),
	eq: entry<'eq'>({
		kind: 'eq',
		category: 'effect',
		label: 'EQ',
		description: '10-band graphic EQ at ISO octave centres (32 Hz → 16 kHz).',
		component: Eq,
		defaultData: { gainsDb: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], bypassed: false }
	}),
	levelMeter: entry<'levelMeter'>({
		kind: 'levelMeter',
		category: 'monitor',
		label: 'Level Meter',
		description: 'Live L/R peak + RMS meter. Works standalone or anywhere in a chain.',
		component: LevelMeter,
		defaultData: {}
	}),
	lufsMeter: entry<'lufsMeter'>({
		kind: 'lufsMeter',
		category: 'monitor',
		label: 'LUFS Meter',
		description: 'EBU R128 loudness meter — Momentary / Short-term / Integrated LUFS.',
		component: LufsMeter,
		defaultData: { target: -14 }
	}),
	waveform: entry<'waveform'>({
		kind: 'waveform',
		category: 'monitor',
		label: 'Waveform',
		description: 'Live waveform — filled min/max envelope for L and R channels.',
		component: Waveform,
		defaultData: { segs: 4 }
	}),
	limiter: entry<'limiter'>({
		kind: 'limiter',
		category: 'effect',
		label: 'Limiter',
		description: 'Brick-wall limiter with look-ahead — catches peaks before they emerge, instant attack with exponential release.',
		component: Limiter,
		defaultData: { ceilingDb: -0.3, lookaheadMs: 5, releaseMs: 50, bypassed: false }
	}),
	compressor: entry<'compressor'>({
		kind: 'compressor',
		category: 'effect',
		label: 'Compressor',
		description: 'Threshold/ratio compressor with soft knee, separate attack/release, and makeup gain.',
		component: Compressor,
		defaultData: {
			thresholdDb: -18,
			ratio: 3,
			attackMs: 10,
			releaseMs: 100,
			kneeDb: 6,
			makeupDb: 0,
			bypassed: false
		}
	}),
	noiseGate: entry<'noiseGate'>({
		kind: 'noiseGate',
		category: 'effect',
		label: 'Noise Gate',
		description: 'Closes when input drops below threshold; hold timer prevents chatter on borderline signals.',
		component: NoiseGate,
		defaultData: {
			thresholdDb: -40,
			rangeDb: -40,
			attackMs: 1,
			holdMs: 50,
			releaseMs: 200,
			bypassed: false
		}
	}),
	delay: entry<'delay'>({
		kind: 'delay',
		category: 'effect',
		label: 'Delay',
		description: 'Stereo delay (1-2000 ms) with feedback and dry/wet mix.',
		component: Delay,
		defaultData: { timeMs: 250, feedback: 0.4, mix: 0.35, bypassed: false }
	}),
	reverb: entry<'reverb'>({
		kind: 'reverb',
		category: 'effect',
		label: 'Reverb',
		description: 'Freeverb algorithmic reverb — room size, damping, stereo width, dry/wet mix.',
		component: Reverb,
		defaultData: { roomSize: 0.5, damping: 0.5, width: 1, mix: 0.33, bypassed: false }
	}),
	noiseSuppressor: entry<'noiseSuppressor'>({
		kind: 'noiseSuppressor',
		category: 'effect',
		label: 'Noise Suppressor',
		description: 'DeepFilterNet deep-learning speech denoise. Runs at 48 kHz only; off-rate signals pass through.',
		component: NoiseSuppressor,
		defaultData: { attenuationLimitDb: 100, bypassed: false }
	})
};

export const nodeTypes: NodeTypes = Object.fromEntries(
	Object.entries(registry).map(([kind, entry]) => [kind, entry.component])
);

export const kinds: NodeKind[] = Object.keys(registry) as NodeKind[];

export const categoryOrder: NodeCategory[] = ['input', 'effect', 'monitor', 'output'];

export const categoryLabel: Record<NodeCategory, string> = {
	input: 'Inputs',
	effect: 'Effects',
	monitor: 'Monitors',
	output: 'Outputs'
};

export const kindsByCategory: Record<NodeCategory, NodeKind[]> = categoryOrder.reduce(
	(acc, category) => {
		acc[category] = kinds.filter((k) => registry[k].category === category);
		return acc;
	},
	{} as Record<NodeCategory, NodeKind[]>
);

// Default data must not leak references to the registry copy, otherwise
// independent nodes would share the same object.
export function defaultDataFor(kind: NodeKind): AnyNodeData {
	return { ...registry[kind].defaultData };
}
