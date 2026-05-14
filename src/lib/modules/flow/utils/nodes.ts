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
import Speaker from '../ui/output/speaker.svelte';
import FileRecording from '../ui/output/file_recording.svelte';
import Gain from '../ui/effect/gain.svelte';
import Mute from '../ui/effect/mute.svelte';
import ChannelBalance from '../ui/effect/channel_balance.svelte';
import Saturator from '../ui/effect/saturator.svelte';
import Eq from '../ui/effect/eq.svelte';
import LevelMeter from '../ui/effect/level_meter.svelte';
import LufsMeter from '../ui/effect/lufs_meter.svelte';
import Limiter from '../ui/effect/limiter.svelte';

// MIME type used during drag-and-drop from the sidebar.
export const DND_MIME = 'application/x-betteraudio-nodekind';

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
		description: 'Capture everything the system is playing (ScreenCaptureKit, macOS 13+).',
		component: SystemAudio,
		defaultData: { excludeCurrentApp: true }
	}),
	appAudio: entry<'appAudio'>({
		kind: 'appAudio',
		category: 'input',
		label: 'App Audio',
		description: 'Capture audio from a single running application.',
		component: AppAudio,
		defaultData: { bundleId: null }
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
		description: 'Record to a WAV file (32-bit float, stereo, lossless).',
		component: FileRecording,
		defaultData: { filePath: null }
	}),
	gain: entry<'gain'>({
		kind: 'gain',
		category: 'effect',
		label: 'Gain',
		description: 'Linear amplitude scaling in dB.',
		component: Gain,
		defaultData: { gainDb: 0 }
	}),
	mute: entry<'mute'>({
		kind: 'mute',
		category: 'effect',
		label: 'Mute',
		description: 'Silence the signal.',
		component: Mute,
		defaultData: { muted: false }
	}),
	channelBalance: entry<'channelBalance'>({
		kind: 'channelBalance',
		category: 'effect',
		label: 'Channel Balance',
		description: 'Separate gain for left and right channels.',
		component: ChannelBalance,
		defaultData: { leftGainDb: 0, rightGainDb: 0 }
	}),
	saturator: entry<'saturator'>({
		kind: 'saturator',
		category: 'effect',
		label: 'Saturator',
		description: 'Soft tanh saturator — smooth distortion, no hard clipping. Not a brick-wall limiter.',
		component: Saturator,
		defaultData: { thresholdDb: -0.3, driveDb: 0 }
	}),
	eq: entry<'eq'>({
		kind: 'eq',
		category: 'effect',
		label: 'EQ',
		description: '10-band graphic EQ at ISO octave centres (32 Hz → 16 kHz).',
		component: Eq,
		defaultData: { gainsDb: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] }
	}),
	levelMeter: entry<'levelMeter'>({
		kind: 'levelMeter',
		category: 'effect',
		label: 'Level Meter',
		description: 'Live L/R peak + RMS meter. Pass-through — drop it anywhere in a chain to watch the signal.',
		component: LevelMeter,
		defaultData: {}
	}),
	lufsMeter: entry<'lufsMeter'>({
		kind: 'lufsMeter',
		category: 'effect',
		label: 'LUFS Meter',
		description: 'EBU R128 loudness meter — Momentary / Short-term / Integrated LUFS. Pass-through.',
		component: LufsMeter,
		defaultData: { target: -14 }
	}),
	limiter: entry<'limiter'>({
		kind: 'limiter',
		category: 'effect',
		label: 'Limiter',
		description: 'Brick-wall limiter with look-ahead — catches peaks before they emerge, instant attack with exponential release.',
		component: Limiter,
		defaultData: { ceilingDb: -0.3, lookaheadMs: 5, releaseMs: 50 }
	})
};

export const nodeTypes: NodeTypes = Object.fromEntries(
	Object.entries(registry).map(([kind, entry]) => [kind, entry.component])
);

export const kinds: NodeKind[] = Object.keys(registry) as NodeKind[];

export const categoryOrder: NodeCategory[] = ['input', 'effect', 'output'];

export const categoryLabel: Record<NodeCategory, string> = {
	input: 'Inputs',
	effect: 'Effects',
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
