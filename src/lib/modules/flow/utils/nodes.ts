import type { Component } from 'svelte';
import type { ClassValue } from 'svelte/elements';
import type { NodeTypes } from '@xyflow/svelte';
import { DEFAULT_NODE_DATA } from '$lib/modules/pipeline/defaults';
import {
	Apps as AppsIcon,
	ArrowDownload as ArrowDownloadIcon,
	ArrowUpload as ArrowUploadIcon,
	Balance as BalanceIcon,
	Compressor as CompressorIcon,
	DataBar as DataBarIcon,
	Delay as DelayIcon,
	Gauge as GaugeIcon,
	Limiter as LimiterIcon,
	Mic as MicIcon,
	MusicNote as MusicNoteIcon,
	NoiseGate as NoiseGateIcon,
	PeopleTeam as PeopleTeamIcon,
	Pulse as PulseIcon,
	FileRecord as FileRecordIcon,
	Reverb as ReverbIcon,
	Saturator as SaturatorIcon,
	Sliders as SlidersIcon,
	SoundWave as SoundWaveIcon,
	Speaker as SpeakerIcon,
	SpeakerMute as SpeakerMuteIcon,
	Trending as TrendingIcon,
	Wand as WandIcon
} from '$lib/components/icons';
import type { AnyNodeData, NodeCategory, NodeDataMap, NodeKind } from '$lib/modules/pipeline/types';
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
import WebRtcCollaborator from '../ui/effect/webrtc_collaborator.svelte';
import NetReceiver from '../ui/input/net_receiver.svelte';
import NetSender from '../ui/output/net_sender.svelte';

// MIME type used during drag-and-drop from the sidebar.
export const DND_MIME = 'application/x-splitwave-nodekind';

export const PREVIEW_CTX = Symbol('flow-preview');

export interface NodeRegistryEntry<K extends NodeKind = NodeKind> {
	kind: K;
	category: NodeCategory;
	label: string;
	description: string;
	component: Component<any>;
	icon: Component<{ class?: ClassValue; title?: string }>;
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
		icon: MicIcon,
		defaultData: DEFAULT_NODE_DATA['microphone']
	}),
	systemAudio: entry<'systemAudio'>({
		kind: 'systemAudio',
		category: 'input',
		label: 'System Audio',
		description: 'Capture everything the system is playing.',
		component: SystemAudio,
		icon: SoundWaveIcon,
		defaultData: DEFAULT_NODE_DATA['systemAudio']
	}),
	appAudio: entry<'appAudio'>({
		kind: 'appAudio',
		category: 'input',
		label: 'App Audio',
		description: 'Capture audio from a single running application.',
		component: AppAudio,
		icon: AppsIcon,
		defaultData: DEFAULT_NODE_DATA['appAudio']
	}),
	audioFile: entry<'audioFile'>({
		kind: 'audioFile',
		category: 'input',
		label: 'Audio File',
		description:
			'Play a WAV file as a source. With no live inputs the pipeline runs faster than real time.',
		component: AudioFile,
		icon: MusicNoteIcon,
		defaultData: DEFAULT_NODE_DATA['audioFile']
	}),
	speaker: entry<'speaker'>({
		kind: 'speaker',
		category: 'output',
		label: 'Speaker',
		description: 'Route audio to a physical output (built-in speakers, headphones, interface).',
		component: Speaker,
		icon: SpeakerIcon,
		defaultData: DEFAULT_NODE_DATA['speaker']
	}),
	fileRecording: entry<'fileRecording'>({
		kind: 'fileRecording',
		category: 'output',
		label: 'File Recording',
		description: 'Record to WAV / FLAC / AIFF (lossless), or Opus / MP3 / AAC (lossy).',
		component: FileRecording,
		icon: FileRecordIcon,
		defaultData: DEFAULT_NODE_DATA['fileRecording']
	}),
	gain: entry<'gain'>({
		kind: 'gain',
		category: 'effect',
		label: 'Gain',
		description: 'Linear amplitude scaling in dB.',
		component: Gain,
		icon: TrendingIcon,
		defaultData: DEFAULT_NODE_DATA['gain']
	}),
	mute: entry<'mute'>({
		kind: 'mute',
		category: 'effect',
		label: 'Mute',
		description: 'Silence the signal.',
		component: Mute,
		icon: SpeakerMuteIcon,
		defaultData: DEFAULT_NODE_DATA['mute']
	}),
	channelBalance: entry<'channelBalance'>({
		kind: 'channelBalance',
		category: 'effect',
		label: 'Channel Balance',
		description: 'Separate gain for left and right channels.',
		component: ChannelBalance,
		icon: BalanceIcon,
		defaultData: DEFAULT_NODE_DATA['channelBalance']
	}),
	saturator: entry<'saturator'>({
		kind: 'saturator',
		category: 'effect',
		label: 'Saturator',
		description:
			'Soft tanh saturator — smooth distortion, no hard clipping. Not a brick-wall limiter.',
		component: Saturator,
		icon: SaturatorIcon,
		defaultData: DEFAULT_NODE_DATA['saturator']
	}),
	eq: entry<'eq'>({
		kind: 'eq',
		category: 'effect',
		label: 'EQ',
		description: '10-band graphic EQ at ISO octave centres (32 Hz → 16 kHz).',
		component: Eq,
		icon: SlidersIcon,
		defaultData: DEFAULT_NODE_DATA['eq']
	}),
	levelMeter: entry<'levelMeter'>({
		kind: 'levelMeter',
		category: 'monitor',
		label: 'Level Meter',
		description: 'Live L/R peak + RMS meter. Works standalone or anywhere in a chain.',
		component: LevelMeter,
		icon: DataBarIcon,
		defaultData: DEFAULT_NODE_DATA['levelMeter']
	}),
	lufsMeter: entry<'lufsMeter'>({
		kind: 'lufsMeter',
		category: 'monitor',
		label: 'Loudness',
		description:
			'EBU R128 loudness — M/S/I LUFS, True Peak, Loudness Range, PLR, Dynamic Range.',
		component: LufsMeter,
		icon: GaugeIcon,
		defaultData: DEFAULT_NODE_DATA['lufsMeter']
	}),
	waveform: entry<'waveform'>({
		kind: 'waveform',
		category: 'monitor',
		label: 'Waveform',
		description: 'Live waveform — filled min/max envelope for L and R channels.',
		component: Waveform,
		icon: PulseIcon,
		defaultData: DEFAULT_NODE_DATA['waveform']
	}),
	limiter: entry<'limiter'>({
		kind: 'limiter',
		category: 'effect',
		label: 'Limiter',
		description:
			'Brick-wall limiter with look-ahead — catches peaks before they emerge, instant attack with exponential release.',
		component: Limiter,
		icon: LimiterIcon,
		defaultData: DEFAULT_NODE_DATA['limiter']
	}),
	compressor: entry<'compressor'>({
		kind: 'compressor',
		category: 'effect',
		label: 'Compressor',
		description:
			'Threshold/ratio compressor with soft knee, separate attack/release, and makeup gain.',
		component: Compressor,
		icon: CompressorIcon,
		defaultData: DEFAULT_NODE_DATA['compressor']
	}),
	noiseGate: entry<'noiseGate'>({
		kind: 'noiseGate',
		category: 'effect',
		label: 'Noise Gate',
		description:
			'Closes when input drops below threshold; hold timer prevents chatter on borderline signals.',
		component: NoiseGate,
		icon: NoiseGateIcon,
		defaultData: DEFAULT_NODE_DATA['noiseGate']
	}),
	delay: entry<'delay'>({
		kind: 'delay',
		category: 'effect',
		label: 'Delay',
		description: 'Stereo delay (1-2000 ms) with feedback and dry/wet mix.',
		component: Delay,
		icon: DelayIcon,
		defaultData: DEFAULT_NODE_DATA['delay']
	}),
	reverb: entry<'reverb'>({
		kind: 'reverb',
		category: 'effect',
		label: 'Reverb',
		description: 'Freeverb algorithmic reverb — room size, damping, stereo width, dry/wet mix.',
		component: Reverb,
		icon: ReverbIcon,
		defaultData: DEFAULT_NODE_DATA['reverb']
	}),
	noiseSuppressor: entry<'noiseSuppressor'>({
		kind: 'noiseSuppressor',
		category: 'effect',
		label: 'Noise Suppressor',
		description:
			'DeepFilterNet deep-learning speech denoise. Model runs at 48 kHz mono; resampled and downmixed for you.',
		component: NoiseSuppressor,
		icon: WandIcon,
		defaultData: DEFAULT_NODE_DATA['noiseSuppressor']
	}),
	webRtcCollaborator: entry<'webRtcCollaborator'>({
		kind: 'webRtcCollaborator',
		category: 'network',
		label: 'WebRTC',
		description:
			'Collaborate over WebRTC — send this signal to remote peers and route each peer back into the graph.',
		component: WebRtcCollaborator,
		icon: PeopleTeamIcon,
		defaultData: DEFAULT_NODE_DATA['webRtcCollaborator']
	}),
	netReceiver: entry<'netReceiver'>({
		kind: 'netReceiver',
		category: 'network',
		label: 'Net Receiver',
		description: 'Receive audio over UDP from a direct IP sender (Opus or raw PCM).',
		component: NetReceiver,
		icon: ArrowDownloadIcon,
		defaultData: DEFAULT_NODE_DATA['netReceiver']
	}),
	netSender: entry<'netSender'>({
		kind: 'netSender',
		category: 'network',
		label: 'Net Sender',
		description: 'Send audio over UDP to a direct IP target (Opus or raw PCM).',
		component: NetSender,
		icon: ArrowUploadIcon,
		defaultData: DEFAULT_NODE_DATA['netSender']
	})
};

export const nodeTypes: NodeTypes = Object.fromEntries(
	Object.entries(registry).map(([kind, entry]) => [kind, entry.component])
);

export const kinds: NodeKind[] = Object.keys(registry) as NodeKind[];

export const categoryOrder: NodeCategory[] = ['input', 'output', 'monitor', 'network', 'effect'];

export const categoryLabel: Record<NodeCategory, string> = {
	input: 'Inputs',
	output: 'Outputs',
	monitor: 'Monitors',
	network: 'Network',
	effect: 'Effects'
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
	// Deep: nested values (an EQ's gain array) would otherwise be shared with
	// every other node of the same kind.
	return structuredClone(registry[kind].defaultData);
}
