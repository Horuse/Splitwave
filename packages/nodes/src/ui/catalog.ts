import type { Component } from 'svelte';
import type { ClassValue } from 'svelte/elements';
import { DEFAULT_NODE_DATA } from '../defaults';
import AppsIcon from '~icons/fluent/apps-16-filled';
import ArrowDownloadIcon from '~icons/fluent/arrow-download-16-filled';
import BackspaceIcon from '~icons/fluent/backspace-24-filled';
import ArrowUploadIcon from '~icons/fluent/arrow-upload-16-filled';
import ArrowRightIcon from '~icons/fluent/arrow-right-16-filled';
import ArrowSwapIcon from '~icons/fluent/arrow-swap-16-filled';
import ChevronDoubleRightIcon from '~icons/fluent/chevron-double-right-20-filled';
import DataBarIcon from '~icons/fluent/data-bar-vertical-16-filled';
import GaugeIcon from '~icons/fluent/gauge-16-filled';
import MicIcon from '~icons/fluent/mic-16-filled';
import MusicNoteIcon from '~icons/fluent/music-note-2-16-filled';
import PeopleTeamIcon from '~icons/fluent/people-team-16-filled';
import PlugIcon from '~icons/fluent/plug-connected-16-filled';
import PulseIcon from '~icons/fluent/pulse-20-filled';
import KeyCommandIcon from '~icons/fluent/key-command-24-regular';
import LoopIcon from '~icons/fluent/arrow-repeat-all-16-filled';
import ArrowRedoIcon from '~icons/fluent/arrow-redo-16-filled';
import HourglassIcon from '~icons/fluent/hourglass-16-filled';
import SlidersIcon from '~icons/fluent/options-16-filled';
import SoundWaveIcon from '~icons/fluent/sound-wave-circle-16-filled';
import SpeakerIcon from '~icons/fluent/speaker-2-16-filled';
import SpeakerMuteIcon from '~icons/fluent/speaker-mute-16-filled';
import TrendingIcon from '~icons/fluent/arrow-trending-16-filled';
import WandIcon from '~icons/fluent/wand-16-filled';
import {
	Balance as BalanceIcon,
	Compressor as CompressorIcon,
	Delay as DelayIcon,
	Limiter as LimiterIcon,
	NoiseGate as NoiseGateIcon,
	FileRecord as FileRecordIcon,
	Reverb as ReverbIcon,
	Saturator as SaturatorIcon
} from './icons';
import type { AnyNodeData, NodeCategory, NodeDataMap, NodeKind } from '../index';

/** What the host shows about a node kind without rendering it. */
export interface NodeRegistryEntry<K extends NodeKind = NodeKind> {
	kind: K;
	category: NodeCategory;
	label: string;
	description: string;
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
		label: 'Microphone',
		description: 'Capture from a physical input (built-in mic, USB, audio interface).',
		icon: MicIcon,
		defaultData: DEFAULT_NODE_DATA['microphone']
	}),
	systemAudio: entry<'systemAudio'>({
		kind: 'systemAudio',
		category: 'input',
		label: 'System Audio',
		description: 'Capture everything the system is playing.',
		icon: SoundWaveIcon,
		defaultData: DEFAULT_NODE_DATA['systemAudio']
	}),
	appAudio: entry<'appAudio'>({
		kind: 'appAudio',
		category: 'input',
		label: 'App Audio',
		description: 'Capture audio from a single running application.',
		icon: AppsIcon,
		defaultData: DEFAULT_NODE_DATA['appAudio']
	}),
	audioFile: entry<'audioFile'>({
		kind: 'audioFile',
		category: 'input',
		label: 'Audio File',
		description: 'Play a WAV file as a source. With no live inputs the pipeline runs faster than real time.',
		icon: MusicNoteIcon,
		defaultData: DEFAULT_NODE_DATA['audioFile']
	}),
	speaker: entry<'speaker'>({
		kind: 'speaker',
		category: 'output',
		label: 'Speaker',
		description: 'Route audio to a physical output (built-in speakers, headphones, interface).',
		icon: SpeakerIcon,
		defaultData: DEFAULT_NODE_DATA['speaker']
	}),
	fileRecording: entry<'fileRecording'>({
		kind: 'fileRecording',
		category: 'output',
		label: 'File Recording',
		description: 'Record to WAV / FLAC / AIFF (lossless), or Opus / MP3 / AAC (lossy; AAC macOS-only).',
		icon: FileRecordIcon,
		defaultData: DEFAULT_NODE_DATA['fileRecording']
	}),
	gain: entry<'gain'>({
		kind: 'gain',
		category: 'effect',
		label: 'Gain',
		description: 'Linear amplitude scaling in dB.',
		icon: TrendingIcon,
		defaultData: DEFAULT_NODE_DATA['gain']
	}),
	mute: entry<'mute'>({
		kind: 'mute',
		category: 'routing',
		label: 'Mute',
		description: 'Silence the signal.',
		icon: SpeakerMuteIcon,
		defaultData: DEFAULT_NODE_DATA['mute']
	}),
	channelBalance: entry<'channelBalance'>({
		kind: 'channelBalance',
		category: 'effect',
		label: 'Channel Balance',
		description: 'Separate gain for left and right channels.',
		icon: BalanceIcon,
		defaultData: DEFAULT_NODE_DATA['channelBalance']
	}),
	saturator: entry<'saturator'>({
		kind: 'saturator',
		category: 'effect',
		label: 'Saturator',
		description: 'Soft tanh saturator — smooth distortion, no hard clipping. Not a brick-wall limiter.',
		icon: SaturatorIcon,
		defaultData: DEFAULT_NODE_DATA['saturator']
	}),
	eq: entry<'eq'>({
		kind: 'eq',
		category: 'effect',
		label: 'EQ',
		description: '10-band graphic EQ at ISO octave centres (32 Hz → 16 kHz).',
		icon: SlidersIcon,
		defaultData: DEFAULT_NODE_DATA['eq']
	}),
	levelMeter: entry<'levelMeter'>({
		kind: 'levelMeter',
		category: 'monitor',
		label: 'Level Meter',
		description: 'Live L/R peak + RMS meter. Works standalone or anywhere in a chain.',
		icon: DataBarIcon,
		defaultData: DEFAULT_NODE_DATA['levelMeter']
	}),
	lufsMeter: entry<'lufsMeter'>({
		kind: 'lufsMeter',
		category: 'monitor',
		label: 'Loudness',
		description: 'EBU R128 loudness — M/S/I LUFS, True Peak, Loudness Range, PLR, Dynamic Range.',
		icon: GaugeIcon,
		defaultData: DEFAULT_NODE_DATA['lufsMeter']
	}),
	waveform: entry<'waveform'>({
		kind: 'waveform',
		category: 'monitor',
		label: 'Waveform',
		description: 'Live waveform — filled min/max envelope for L and R channels.',
		icon: PulseIcon,
		defaultData: DEFAULT_NODE_DATA['waveform'],
		defaultSize: { width: 256, height: 130 }
	}),
	spectrum: entry<'spectrum'>({
		kind: 'spectrum',
		category: 'monitor',
		label: 'Spectrum',
		description: 'Live spectrum analyzer — log-frequency FFT magnitude with a peak-hold fall-off.',
		icon: DataBarIcon,
		defaultData: DEFAULT_NODE_DATA['spectrum']
	}),
	limiter: entry<'limiter'>({
		kind: 'limiter',
		category: 'effect',
		label: 'Limiter',
		description: 'Brick-wall limiter with look-ahead — catches peaks before they emerge, instant attack with exponential release.',
		icon: LimiterIcon,
		defaultData: DEFAULT_NODE_DATA['limiter']
	}),
	compressor: entry<'compressor'>({
		kind: 'compressor',
		category: 'effect',
		label: 'Compressor',
		description: 'Threshold/ratio compressor with soft knee, separate attack/release, and makeup gain.',
		icon: CompressorIcon,
		defaultData: DEFAULT_NODE_DATA['compressor']
	}),
	noiseGate: entry<'noiseGate'>({
		kind: 'noiseGate',
		category: 'effect',
		label: 'Noise Gate',
		description: 'Closes when input drops below threshold; hold timer prevents chatter on borderline signals.',
		icon: NoiseGateIcon,
		defaultData: DEFAULT_NODE_DATA['noiseGate']
	}),
	delay: entry<'delay'>({
		kind: 'delay',
		category: 'effect',
		label: 'Delay',
		description: 'Stereo delay (1-2000 ms) with feedback and dry/wet mix.',
		icon: DelayIcon,
		defaultData: DEFAULT_NODE_DATA['delay']
	}),
	reverb: entry<'reverb'>({
		kind: 'reverb',
		category: 'effect',
		label: 'Reverb',
		description: 'Freeverb algorithmic reverb — room size, damping, stereo width, dry/wet mix.',
		icon: ReverbIcon,
		defaultData: DEFAULT_NODE_DATA['reverb']
	}),
	noiseSuppressor: entry<'noiseSuppressor'>({
		kind: 'noiseSuppressor',
		category: 'effect',
		label: 'Noise Suppressor',
		description: 'DeepFilterNet deep-learning speech denoise. Model runs at 48 kHz mono; resampled and downmixed for you.',
		icon: WandIcon,
		defaultData: DEFAULT_NODE_DATA['noiseSuppressor']
	}),
	declick: entry<'declick'>({
		kind: 'declick',
		category: 'effect',
		label: 'Declick',
		description: 'Removes short impulsive clicks and crackle — median detector replaces spikes, clean audio passes through.',
		icon: BackspaceIcon,
		defaultData: DEFAULT_NODE_DATA['declick']
	}),
	deEsser: entry<'deEsser'>({
		kind: 'deEsser',
		category: 'effect',
		label: 'De-esser',
		description: 'Tames harsh "s"/"sh" sibilance — splits off the high band and compresses only that, leaving the voice body untouched.',
		icon: SoundWaveIcon,
		defaultData: DEFAULT_NODE_DATA['deEsser']
	}),
	plugin: entry<'plugin'>({
		kind: 'plugin',
		category: 'effect',
		label: 'Plugin',
		description: 'Host a third-party CLAP audio plugin as a node — scan installed plugins and route channels through it like any effect.',
		icon: PlugIcon,
		defaultData: DEFAULT_NODE_DATA['plugin']
	}),
	webRtcCollaborator: entry<'webRtcCollaborator'>({
		kind: 'webRtcCollaborator',
		category: 'network',
		label: 'WebRTC',
		description: 'Collaborate over WebRTC — send this signal to remote peers and route each peer back into the graph.',
		icon: PeopleTeamIcon,
		defaultData: DEFAULT_NODE_DATA['webRtcCollaborator']
	}),
	netReceiver: entry<'netReceiver'>({
		kind: 'netReceiver',
		category: 'network',
		label: 'Net Receiver',
		description: 'Receive audio over UDP from a direct IP sender (Opus or raw PCM).',
		icon: ArrowDownloadIcon,
		defaultData: DEFAULT_NODE_DATA['netReceiver']
	}),
	netSender: entry<'netSender'>({
		kind: 'netSender',
		category: 'network',
		label: 'Net Sender',
		description: 'Send audio over UDP to a direct IP target (Opus or raw PCM).',
		icon: ArrowUploadIcon,
		defaultData: DEFAULT_NODE_DATA['netSender']
	}),
	keyInput: entry<'keyInput'>({
		kind: 'keyInput',
		category: 'control',
		label: 'Key Input',
		description: 'A global shortcut as a control signal: high while the key is held, even with the app in the background.',
		icon: KeyCommandIcon,
		defaultData: DEFAULT_NODE_DATA['keyInput']
	}),
	toggle: entry<'toggle'>({
		kind: 'toggle',
		category: 'control',
		label: 'Toggle',
		description: 'Flips its output each time the input goes high, and keeps it until the next press.',
		icon: LoopIcon,
		defaultData: DEFAULT_NODE_DATA['toggle']
	}),
	cycle: entry<'cycle'>({
		kind: 'cycle',
		category: 'control',
		label: 'Cycle',
		description: 'Steps through its outputs one press at a time and wraps around; only the current step is high.',
		icon: ArrowRedoIcon,
		defaultData: DEFAULT_NODE_DATA['cycle']
	}),
	timer: entry<'timer'>({
		kind: 'timer',
		category: 'control',
		label: 'Timer',
		description: 'Delay turns its output on a set time after start goes high; Repeat pulses it at that interval while start stays high.',
		icon: HourglassIcon,
		defaultData: DEFAULT_NODE_DATA['timer']
	}),
	logic: entry<'logic'>({
		kind: 'logic',
		category: 'control',
		label: 'Logic',
		description: 'Combines control signals with a logic operator, such as NOT to invert one or AND to need two at once.',
		icon: ArrowRightIcon,
		defaultData: DEFAULT_NODE_DATA['logic']
	}),
	if: entry<'if'>({
		kind: 'if',
		category: 'routing',
		label: 'If',
		description: 'Sends its input to the true output while the condition is high, to the false output otherwise, crossfading between them.',
		icon: ArrowRightIcon,
		defaultData: DEFAULT_NODE_DATA['if']
	}),
	switch: entry<'switch'>({
		kind: 'switch',
		category: 'routing',
		label: 'Switch',
		description: 'Sends its input to one of 2-8 outputs; each output has its own trigger, so one key per branch picks it.',
		icon: ChevronDoubleRightIcon,
		defaultData: DEFAULT_NODE_DATA['switch']
	}),
	select: entry<'select'>({
		kind: 'select',
		category: 'routing',
		label: 'Select',
		description: 'Hears one of 2-8 inputs; each input has its own trigger, so one key per source picks it.',
		icon: ArrowSwapIcon,
		defaultData: DEFAULT_NODE_DATA['select']
	}),
	levelTrigger: entry<'levelTrigger'>({
		kind: 'levelTrigger',
		category: 'control',
		label: 'Level Trigger',
		description: 'High while its input is louder than the threshold, held a moment after it drops so speech does not flicker.',
		icon: GaugeIcon,
		defaultData: DEFAULT_NODE_DATA['levelTrigger']
	})
};


export const kinds: NodeKind[] = Object.keys(registry) as NodeKind[];

export const categoryOrder: NodeCategory[] = ['input', 'output', 'monitor', 'network', 'effect', 'routing', 'control'];

export const categoryLabel: Record<NodeCategory, string> = {
	input: 'Inputs',
	output: 'Outputs',
	monitor: 'Monitors',
	network: 'Network',
	effect: 'Effects',
	control: 'Control',
	routing: 'Routing'
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
