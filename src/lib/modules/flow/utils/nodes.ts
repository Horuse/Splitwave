import type { Component } from 'svelte';
import type { ClassValue } from 'svelte/elements';
import type { NodeTypes } from '@xyflow/svelte';
import { DEFAULT_NODE_DATA } from '$lib/modules/pipeline/defaults';
import {
	Apps as AppsIcon,
	ArrowDownload as ArrowDownloadIcon,
	Backspace as BackspaceIcon,
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
	Plug as PlugIcon,
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
import { t } from '$lib/modules/i18n';
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
import Spectrum from '../ui/effect/spectrum.svelte';
import Limiter from '../ui/effect/limiter.svelte';
import Compressor from '../ui/effect/compressor.svelte';
import NoiseGate from '../ui/effect/noise_gate.svelte';
import Delay from '../ui/effect/delay.svelte';
import Reverb from '../ui/effect/reverb.svelte';
import NoiseSuppressor from '../ui/effect/noise_suppressor.svelte';
import Declick from '../ui/effect/declick.svelte';
import DeEsser from '../ui/effect/de_esser.svelte';
import Plugin from '../ui/effect/plugin.svelte';
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
	/** Fixed spawn size for nodes whose content needs room (resizable after). */
	defaultSize?: { width: number; height: number };
}

function entry<K extends NodeKind>(e: NodeRegistryEntry<K>): NodeRegistryEntry {
	return e as NodeRegistryEntry;
}

export const registry: Record<NodeKind, NodeRegistryEntry> = {
	microphone: entry<'microphone'>({
		kind: 'microphone',
		category: 'input',
		get label() {
			return t('node.microphone.label');
		},
		get description() {
			return t('node.microphone.description');
		},
		component: Microphone,
		icon: MicIcon,
		defaultData: DEFAULT_NODE_DATA['microphone']
	}),
	systemAudio: entry<'systemAudio'>({
		kind: 'systemAudio',
		category: 'input',
		get label() {
			return t('node.systemAudio.label');
		},
		get description() {
			return t('node.systemAudio.description');
		},
		component: SystemAudio,
		icon: SoundWaveIcon,
		defaultData: DEFAULT_NODE_DATA['systemAudio']
	}),
	appAudio: entry<'appAudio'>({
		kind: 'appAudio',
		category: 'input',
		get label() {
			return t('node.appAudio.label');
		},
		get description() {
			return t('node.appAudio.description');
		},
		component: AppAudio,
		icon: AppsIcon,
		defaultData: DEFAULT_NODE_DATA['appAudio']
	}),
	audioFile: entry<'audioFile'>({
		kind: 'audioFile',
		category: 'input',
		get label() {
			return t('node.audioFile.label');
		},
		get description() {
			return t('node.audioFile.description');
		},
		component: AudioFile,
		icon: MusicNoteIcon,
		defaultData: DEFAULT_NODE_DATA['audioFile']
	}),
	speaker: entry<'speaker'>({
		kind: 'speaker',
		category: 'output',
		get label() {
			return t('node.speaker.label');
		},
		get description() {
			return t('node.speaker.description');
		},
		component: Speaker,
		icon: SpeakerIcon,
		defaultData: DEFAULT_NODE_DATA['speaker']
	}),
	fileRecording: entry<'fileRecording'>({
		kind: 'fileRecording',
		category: 'output',
		get label() {
			return t('node.fileRecording.label');
		},
		get description() {
			return t('node.fileRecording.description');
		},
		component: FileRecording,
		icon: FileRecordIcon,
		defaultData: DEFAULT_NODE_DATA['fileRecording']
	}),
	gain: entry<'gain'>({
		kind: 'gain',
		category: 'effect',
		get label() {
			return t('node.gain.label');
		},
		get description() {
			return t('node.gain.description');
		},
		component: Gain,
		icon: TrendingIcon,
		defaultData: DEFAULT_NODE_DATA['gain']
	}),
	mute: entry<'mute'>({
		kind: 'mute',
		category: 'effect',
		get label() {
			return t('node.mute.label');
		},
		get description() {
			return t('node.mute.description');
		},
		component: Mute,
		icon: SpeakerMuteIcon,
		defaultData: DEFAULT_NODE_DATA['mute']
	}),
	channelBalance: entry<'channelBalance'>({
		kind: 'channelBalance',
		category: 'effect',
		get label() {
			return t('node.channelBalance.label');
		},
		get description() {
			return t('node.channelBalance.description');
		},
		component: ChannelBalance,
		icon: BalanceIcon,
		defaultData: DEFAULT_NODE_DATA['channelBalance']
	}),
	saturator: entry<'saturator'>({
		kind: 'saturator',
		category: 'effect',
		get label() {
			return t('node.saturator.label');
		},
		get description() {
			return t('node.saturator.description');
		},
		component: Saturator,
		icon: SaturatorIcon,
		defaultData: DEFAULT_NODE_DATA['saturator']
	}),
	eq: entry<'eq'>({
		kind: 'eq',
		category: 'effect',
		get label() {
			return t('node.eq.label');
		},
		get description() {
			return t('node.eq.description');
		},
		component: Eq,
		icon: SlidersIcon,
		defaultData: DEFAULT_NODE_DATA['eq']
	}),
	levelMeter: entry<'levelMeter'>({
		kind: 'levelMeter',
		category: 'monitor',
		get label() {
			return t('node.levelMeter.label');
		},
		get description() {
			return t('node.levelMeter.description');
		},
		component: LevelMeter,
		icon: DataBarIcon,
		defaultData: DEFAULT_NODE_DATA['levelMeter']
	}),
	lufsMeter: entry<'lufsMeter'>({
		kind: 'lufsMeter',
		category: 'monitor',
		get label() {
			return t('node.lufsMeter.label');
		},
		get description() {
			return t('node.lufsMeter.description');
		},
		component: LufsMeter,
		icon: GaugeIcon,
		defaultData: DEFAULT_NODE_DATA['lufsMeter']
	}),
	waveform: entry<'waveform'>({
		kind: 'waveform',
		category: 'monitor',
		get label() {
			return t('node.waveform.label');
		},
		get description() {
			return t('node.waveform.description');
		},
		component: Waveform,
		icon: PulseIcon,
		defaultData: DEFAULT_NODE_DATA['waveform'],
		defaultSize: { width: 256, height: 130 }
	}),
	spectrum: entry<'spectrum'>({
		kind: 'spectrum',
		category: 'monitor',
		get label() {
			return t('node.spectrum.label');
		},
		get description() {
			return t('node.spectrum.description');
		},
		component: Spectrum,
		icon: DataBarIcon,
		defaultData: DEFAULT_NODE_DATA['spectrum']
	}),
	limiter: entry<'limiter'>({
		kind: 'limiter',
		category: 'effect',
		get label() {
			return t('node.limiter.label');
		},
		get description() {
			return t('node.limiter.description');
		},
		component: Limiter,
		icon: LimiterIcon,
		defaultData: DEFAULT_NODE_DATA['limiter']
	}),
	compressor: entry<'compressor'>({
		kind: 'compressor',
		category: 'effect',
		get label() {
			return t('node.compressor.label');
		},
		get description() {
			return t('node.compressor.description');
		},
		component: Compressor,
		icon: CompressorIcon,
		defaultData: DEFAULT_NODE_DATA['compressor']
	}),
	noiseGate: entry<'noiseGate'>({
		kind: 'noiseGate',
		category: 'effect',
		get label() {
			return t('node.noiseGate.label');
		},
		get description() {
			return t('node.noiseGate.description');
		},
		component: NoiseGate,
		icon: NoiseGateIcon,
		defaultData: DEFAULT_NODE_DATA['noiseGate']
	}),
	delay: entry<'delay'>({
		kind: 'delay',
		category: 'effect',
		get label() {
			return t('node.delay.label');
		},
		get description() {
			return t('node.delay.description');
		},
		component: Delay,
		icon: DelayIcon,
		defaultData: DEFAULT_NODE_DATA['delay']
	}),
	reverb: entry<'reverb'>({
		kind: 'reverb',
		category: 'effect',
		get label() {
			return t('node.reverb.label');
		},
		get description() {
			return t('node.reverb.description');
		},
		component: Reverb,
		icon: ReverbIcon,
		defaultData: DEFAULT_NODE_DATA['reverb']
	}),
	noiseSuppressor: entry<'noiseSuppressor'>({
		kind: 'noiseSuppressor',
		category: 'effect',
		get label() {
			return t('node.noiseSuppressor.label');
		},
		get description() {
			return t('node.noiseSuppressor.description');
		},
		component: NoiseSuppressor,
		icon: WandIcon,
		defaultData: DEFAULT_NODE_DATA['noiseSuppressor']
	}),
	declick: entry<'declick'>({
		kind: 'declick',
		category: 'effect',
		get label() {
			return t('node.declick.label');
		},
		get description() {
			return t('node.declick.description');
		},
		component: Declick,
		icon: BackspaceIcon,
		defaultData: DEFAULT_NODE_DATA['declick']
	}),
	deEsser: entry<'deEsser'>({
		kind: 'deEsser',
		category: 'effect',
		get label() {
			return t('node.deEsser.label');
		},
		get description() {
			return t('node.deEsser.description');
		},
		component: DeEsser,
		icon: SoundWaveIcon,
		defaultData: DEFAULT_NODE_DATA['deEsser']
	}),
	plugin: entry<'plugin'>({
		kind: 'plugin',
		category: 'effect',
		get label() {
			return t('node.plugin.label');
		},
		get description() {
			return t('node.plugin.description');
		},
		component: Plugin,
		icon: PlugIcon,
		defaultData: DEFAULT_NODE_DATA['plugin']
	}),
	webRtcCollaborator: entry<'webRtcCollaborator'>({
		kind: 'webRtcCollaborator',
		category: 'network',
		get label() {
			return t('node.webRtcCollaborator.label');
		},
		get description() {
			return t('node.webRtcCollaborator.description');
		},
		component: WebRtcCollaborator,
		icon: PeopleTeamIcon,
		defaultData: DEFAULT_NODE_DATA['webRtcCollaborator']
	}),
	netReceiver: entry<'netReceiver'>({
		kind: 'netReceiver',
		category: 'network',
		get label() {
			return t('node.netReceiver.label');
		},
		get description() {
			return t('node.netReceiver.description');
		},
		component: NetReceiver,
		icon: ArrowDownloadIcon,
		defaultData: DEFAULT_NODE_DATA['netReceiver']
	}),
	netSender: entry<'netSender'>({
		kind: 'netSender',
		category: 'network',
		get label() {
			return t('node.netSender.label');
		},
		get description() {
			return t('node.netSender.description');
		},
		component: NetSender,
		icon: ArrowUploadIcon,
		defaultData: DEFAULT_NODE_DATA['netSender']
	})
};

export const nodeTypes: NodeTypes = Object.fromEntries(Object.entries(registry).map(([kind, entry]) => [kind, entry.component]));

export const kinds: NodeKind[] = Object.keys(registry) as NodeKind[];

export const categoryOrder: NodeCategory[] = ['input', 'output', 'monitor', 'network', 'effect'];

export const categoryLabel: Record<NodeCategory, string> = {
	get input() {
		return t('category.input');
	},
	get output() {
		return t('category.output');
	},
	get monitor() {
		return t('category.monitor');
	},
	get network() {
		return t('category.network');
	},
	get effect() {
		return t('category.effect');
	}
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
