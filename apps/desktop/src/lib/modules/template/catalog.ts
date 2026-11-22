import type { Template } from './types';
import { CONTROL_OUT, OPEN_HANDLE, SIDECHAIN_HANDLE } from '@splitwave/nodes/ui';

const COL = 300;
const ROW = 280;

export const TEMPLATES: Template[] = [
	{
		id: 'blank',
		accent: 'neutral',
		name: 'Blank',
		description: 'An empty canvas.',
		nodes: [],
		edges: []
	},
	{
		id: 'push-to-talk',
		accent: 'emerald',
		name: 'Push to talk',
		description: 'Hold a key to talk. Key Input opens a Mute between your mic and the output, even while a game is focused.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'key', kind: 'keyInput', position: { x: 0, y: ROW } },
			{ key: 'mute', kind: 'mute', position: { x: COL, y: 0 } },
			{ key: 'spk', kind: 'speaker', position: { x: COL * 2, y: 0 } }
		],
		edges: [
			{ from: 'mic', to: 'mute' },
			{ from: 'mute', to: 'spk' },
			{ from: 'key', to: 'mute', fromHandle: CONTROL_OUT, toHandle: OPEN_HANDLE }
		]
	},
	{
		id: 'voice-ducking',
		accent: 'sky',
		name: 'Voice ducking',
		description: 'Talking automatically lowers app audio, so game or music volume drops while you speak.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'app', kind: 'appAudio', position: { x: 0, y: ROW } },
			{
				key: 'comp',
				kind: 'compressor',
				position: { x: COL, y: ROW * 0.5 },
				data: {
					thresholdDb: -35,
					ratio: 5,
					attackMs: 100,
					releaseMs: 1000,
					kneeDb: 0,
					makeupDb: 0
				}
			},
			{ key: 'spk', kind: 'speaker', position: { x: COL * 2, y: ROW * 0.5 } }
		],
		edges: [
			{ from: 'mic', to: 'comp', toHandle: SIDECHAIN_HANDLE },
			{ from: 'app', to: 'comp', fromCh: 1, toCh: 1 },
			{ from: 'app', to: 'comp', fromCh: 2, toCh: 2 },
			{ from: 'comp', to: 'spk', fromCh: 1, toCh: 1 },
			{ from: 'comp', to: 'spk', fromCh: 2, toCh: 2 }
		]
	},
	{
		id: 'safe-game-audio',
		accent: 'violet',
		name: 'Safe game audio',
		description: 'App audio capped by a limiter so sudden loud sounds never blast your speakers.',
		nodes: [
			{ key: 'app', kind: 'appAudio', position: { x: 0, y: 0 } },
			{ key: 'lim', kind: 'limiter', position: { x: COL, y: 0 } },
			{ key: 'spk', kind: 'speaker', position: { x: COL * 2, y: 0 } }
		],
		edges: [
			{ from: 'app', to: 'lim', fromCh: 1, toCh: 1 },
			{ from: 'app', to: 'lim', fromCh: 2, toCh: 2 },
			{ from: 'lim', to: 'spk', fromCh: 1, toCh: 1 },
			{ from: 'lim', to: 'spk', fromCh: 2, toCh: 2 }
		]
	},
	{
		id: 'clean-mic',
		accent: 'amber',
		name: 'Clean mic',
		description: 'Background noise removed, pauses silenced, level evened out and peaks caught before your virtual mic.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'ns', kind: 'noiseSuppressor', position: { x: COL, y: 0 } },
			{ key: 'gate', kind: 'noiseGate', position: { x: COL * 2, y: 0 } },
			{ key: 'comp', kind: 'compressor', position: { x: COL * 3, y: 0 } },
			{ key: 'lim', kind: 'limiter', position: { x: COL * 4, y: 0 } },
			{ key: 'spk', kind: 'speaker', position: { x: COL * 5, y: 0 } }
		],
		edges: [
			{ from: 'mic', to: 'ns' },
			{ from: 'ns', to: 'gate' },
			{ from: 'gate', to: 'comp' },
			{ from: 'comp', to: 'lim' },
			{ from: 'lim', to: 'spk' }
		]
	},
	{
		id: 'full-voice-and-ducking',
		accent: 'rose',
		name: 'Full voice + ducking',
		description: 'Mic denoised and gated, app audio ducked while you speak and limited so it never overloads.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'ns', kind: 'noiseSuppressor', position: { x: COL, y: 0 } },
			{ key: 'gate', kind: 'noiseGate', position: { x: COL * 2, y: 0 } },

			{ key: 'app', kind: 'appAudio', position: { x: 0, y: ROW * 1.6 } },
			{ key: 'comp', kind: 'compressor', position: { x: COL, y: ROW * 1.6 } },
			{ key: 'lim', kind: 'limiter', position: { x: COL * 2, y: ROW * 1.6 } },

			{ key: 'spk', kind: 'speaker', position: { x: COL * 3, y: ROW * 0.8 } }
		],
		edges: [
			{ from: 'mic', to: 'ns' },
			{ from: 'ns', to: 'gate' },
			{ from: 'gate', to: 'spk', fromCh: 1, toCh: 1 },
			{ from: 'gate', to: 'spk', fromCh: 1, toCh: 2 },
			{ from: 'ns', to: 'comp', toHandle: SIDECHAIN_HANDLE },

			{ from: 'app', to: 'comp', fromCh: 1, toCh: 1 },
			{ from: 'app', to: 'comp', fromCh: 2, toCh: 2 },
			{ from: 'comp', to: 'lim', fromCh: 1, toCh: 1 },
			{ from: 'comp', to: 'lim', fromCh: 2, toCh: 2 },
			{ from: 'lim', to: 'spk', fromCh: 1, toCh: 1 },
			{ from: 'lim', to: 'spk', fromCh: 2, toCh: 2 }
		]
	}
];
