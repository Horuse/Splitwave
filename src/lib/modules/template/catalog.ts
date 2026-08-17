import type { Template } from './types';

const COL = 300;
const ROW = 280;

export const TEMPLATES: Template[] = [
	{
		id: 'blank',
		version: 1,
		accent: 'neutral',
		name: 'Blank',
		description: 'An empty canvas.',
		nodes: [],
		edges: []
	},
	{
		id: 'push-to-talk',
		version: 1,
		accent: 'emerald',
		name: 'Push to talk',
		description: 'Mic straight to your virtual mic, muted by default so you decide when to speak.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'mute', kind: 'mute', position: { x: COL, y: 0 } },
			{ key: 'spk', kind: 'speaker', position: { x: COL * 2, y: 0 } }
		],
		edges: [
			{ from: 'mic', to: 'mute' },
			{ from: 'mute', to: 'spk' }
		]
	},
	{
		id: 'voice-ducking',
		version: 1,
		accent: 'sky',
		name: 'Voice ducking',
		description: 'Talking automatically lowers app audio, so game or music volume drops while you speak.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'mute', kind: 'mute', position: { x: COL, y: 0 } },

			{ key: 'app', kind: 'appAudio', position: { x: 0, y: ROW * 1.6 } },
			{
				key: 'comp',
				kind: 'compressor',
				position: { x: COL, y: ROW * 1.6 },
				data: {
					thresholdDb: -35,
					ratio: 5,
					attackMs: 100,
					releaseMs: 1000,
					kneeDb: 0,
					makeupDb: 0
				}
			},

			{ key: 'spk', kind: 'speaker', position: { x: COL * 2, y: ROW * 0.8 } }
		],
		edges: [
			{ from: 'mic', to: 'mute' },
			{ from: 'mute', to: 'spk', fromCh: 1, toCh: 1 },
			{ from: 'mute', to: 'spk', fromCh: 1, toCh: 2 },
			{ from: 'mic', to: 'comp', toHandle: 'sidechain' },

			{ from: 'app', to: 'comp', fromCh: 1, toCh: 1 },
			{ from: 'app', to: 'comp', fromCh: 2, toCh: 2 },
			{ from: 'comp', to: 'spk', fromCh: 1, toCh: 1 },
			{ from: 'comp', to: 'spk', fromCh: 2, toCh: 2 }
		]
	},
	{
		id: 'safe-game-audio',
		version: 1,
		accent: 'violet',
		name: 'Safe game audio',
		description: 'App audio capped by a limiter so sudden loud sounds never blast your speakers.',
		nodes: [
			{ key: 'app', kind: 'appAudio', position: { x: 0, y: 0 } },
			{ key: 'lim', kind: 'limiter', position: { x: COL, y: 0 } },
			{ key: 'spk', kind: 'speaker', position: { x: COL * 2, y: 0 } }
		],
		edges: [
			{ from: 'app', to: 'lim' },
			{ from: 'lim', to: 'spk' }
		]
	},
	{
		id: 'clean-mic',
		version: 1,
		accent: 'amber',
		name: 'Clean mic',
		description: 'Mic with clicks and background noise stripped before it reaches your virtual mic.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'declick', kind: 'declick', position: { x: COL, y: 0 } },
			{ key: 'gate', kind: 'noiseGate', position: { x: COL * 2, y: 0 } },
			{ key: 'spk', kind: 'speaker', position: { x: COL * 3, y: 0 } }
		],
		edges: [
			{ from: 'mic', to: 'declick' },
			{ from: 'declick', to: 'gate' },
			{ from: 'gate', to: 'spk' }
		]
	},
	{
		id: 'spatial_voice_multimic',
		version: 1,
		accent: 'emerald',
		name: 'Spatial Voice — Multi-Mic',
		description: 'A setup-ready microphone array with spatial focus, denoise, compression and voice EQ.',
		nodes: [
			{ key: 'array', kind: 'microphoneArray', position: { x: 0, y: 0 } },
			{ key: 'ns', kind: 'noiseSuppressor', position: { x: COL, y: 0 } },
			{ key: 'comp', kind: 'compressor', position: { x: COL * 2, y: 0 } },
			{
				key: 'eq',
				kind: 'eq',
				position: { x: COL * 3, y: 0 },
				data: { gainsDb: [-3, -2, -1, 0, 1, 2, 2, 1, 0, -1] }
			},
			{ key: 'spk', kind: 'speaker', position: { x: COL * 4, y: 0 } }
		],
		edges: [
			{ from: 'array', to: 'ns' },
			{ from: 'ns', to: 'comp' },
			{ from: 'comp', to: 'eq' },
			{ from: 'eq', to: 'spk' }
		]
	},
	{
		id: 'full-voice-and-ducking',
		version: 1,
		accent: 'rose',
		name: 'Full voice + ducking',
		description: 'Mic denoised and ducking app audio when you speak, app audio limited so it never overloads.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'ns', kind: 'noiseSuppressor', position: { x: COL, y: 0 } },
			{ key: 'comp', kind: 'compressor', position: { x: COL * 2, y: 0 } },

			{ key: 'app', kind: 'appAudio', position: { x: 0, y: ROW * 1.6 } },
			{ key: 'lim', kind: 'limiter', position: { x: COL * 2, y: ROW * 1.6 } },

			{ key: 'spk', kind: 'speaker', position: { x: COL * 3, y: ROW * 0.8 } }
		],
		edges: [
			{ from: 'mic', to: 'ns' },
			{ from: 'ns', to: 'comp' },
			{ from: 'comp', to: 'spk', fromCh: 1, toCh: 1 },
			{ from: 'comp', to: 'spk', fromCh: 1, toCh: 2 },
			{ from: 'app', to: 'comp', toHandle: 'sidechain' },

			{ from: 'app', to: 'lim', fromCh: 1, toCh: 1 },
			{ from: 'app', to: 'lim', fromCh: 2, toCh: 2 },
			{ from: 'lim', to: 'spk', fromCh: 1, toCh: 1 },
			{ from: 'lim', to: 'spk', fromCh: 2, toCh: 2 }
		]
	}
];
