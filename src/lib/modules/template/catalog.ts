import type { Template } from './types';

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
		id: 'podcast',
		accent: 'emerald',
		name: 'Podcast',
		description: 'Two mics, each gated and compressed, recorded to one file with live monitoring.',
		nodes: [
			{ key: 'mic1', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'gate1', kind: 'noiseGate', position: { x: COL, y: 0 } },
			{ key: 'comp1', kind: 'compressor', position: { x: COL * 2, y: 0 } },
			{ key: 'eq1', kind: 'eq', position: { x: COL * 3, y: 0 } },

			{ key: 'mic2', kind: 'microphone', position: { x: 0, y: ROW * 2 } },
			{ key: 'gate2', kind: 'noiseGate', position: { x: COL, y: ROW * 2 } },
			{ key: 'comp2', kind: 'compressor', position: { x: COL * 2, y: ROW * 2 } },
			{ key: 'eq2', kind: 'eq', position: { x: COL * 3, y: ROW * 2 } },

			{
				key: 'rec',
				kind: 'fileRecording',
				position: { x: COL * 4.4, y: ROW * 0.5 },
				data: { channels: 2 }
			},
			{ key: 'spk', kind: 'speaker', position: { x: COL * 4.4, y: ROW * 1.6 } }
		],
		edges: [
			{ from: 'mic1', to: 'gate1' },
			{ from: 'gate1', to: 'comp1' },
			{ from: 'comp1', to: 'eq1' },
			{ from: 'mic2', to: 'gate2' },
			{ from: 'gate2', to: 'comp2' },
			{ from: 'comp2', to: 'eq2' },

			{ from: 'eq1', to: 'rec', toCh: 1 },
			{ from: 'eq2', to: 'rec', toCh: 2 },
			{ from: 'eq1', to: 'spk', toCh: 1 },
			{ from: 'eq2', to: 'spk', toCh: 2 }
		]
	},
	{
		id: 'streaming-voice',
		accent: 'sky',
		name: 'Streaming voice',
		description: 'One mic cleaned up and levelled, with a loudness readout for broadcast targets.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'ns', kind: 'noiseSuppressor', position: { x: COL, y: 0 } },
			{ key: 'comp', kind: 'compressor', position: { x: COL * 2, y: 0 } },
			{ key: 'lim', kind: 'limiter', position: { x: COL * 3, y: 0 } },
			{ key: 'spk', kind: 'speaker', position: { x: COL * 4.2, y: 0 } },
			{ key: 'loud', kind: 'lufsMeter', position: { x: COL * 4.2, y: ROW } }
		],
		edges: [
			{ from: 'mic', to: 'ns' },
			{ from: 'ns', to: 'comp' },
			{ from: 'comp', to: 'lim' },
			{ from: 'lim', to: 'spk' },
			{ from: 'lim', to: 'loud' }
		]
	},
	{
		id: 'record-system-audio',
		accent: 'violet',
		name: 'Record system audio',
		description: 'Everything the system plays, metered and written to a file in stereo.',
		nodes: [
			{ key: 'src', kind: 'systemAudio', position: { x: 0, y: 0 } },
			{ key: 'meter', kind: 'levelMeter', position: { x: COL * 1.2, y: 0 } },
			{
				key: 'rec',
				kind: 'fileRecording',
				position: { x: COL * 2.4, y: 0 },
				data: { channels: 2 }
			}
		],
		edges: [
			{ from: 'src', to: 'meter', fromCh: 1, toCh: 1 },
			{ from: 'src', to: 'meter', fromCh: 2, toCh: 2 },
			{ from: 'meter', to: 'rec', fromCh: 1, toCh: 1 },
			{ from: 'meter', to: 'rec', fromCh: 2, toCh: 2 }
		]
	},
	{
		id: 'record-app-audio',
		accent: 'amber',
		name: 'Record app audio',
		description: 'Capture a single application, metered and written to a file in stereo.',
		nodes: [
			{ key: 'src', kind: 'appAudio', position: { x: 0, y: 0 } },
			{ key: 'meter', kind: 'levelMeter', position: { x: COL * 1.2, y: 0 } },
			{
				key: 'rec',
				kind: 'fileRecording',
				position: { x: COL * 2.4, y: 0 },
				data: { channels: 2 }
			}
		],
		edges: [
			{ from: 'src', to: 'meter', fromCh: 1, toCh: 1 },
			{ from: 'src', to: 'meter', fromCh: 2, toCh: 2 },
			{ from: 'meter', to: 'rec', fromCh: 1, toCh: 1 },
			{ from: 'meter', to: 'rec', fromCh: 2, toCh: 2 }
		]
	},
	{
		id: 'remote-guest',
		accent: 'rose',
		name: 'Remote guest',
		description:
			'Send a processed mic to WebRTC peers. Peer outputs appear once someone connects, so wire the return side yourself.',
		nodes: [
			{ key: 'mic', kind: 'microphone', position: { x: 0, y: 0 } },
			{ key: 'ns', kind: 'noiseSuppressor', position: { x: COL, y: 0 } },
			{ key: 'comp', kind: 'compressor', position: { x: COL * 2, y: 0 } },
			{ key: 'rtc', kind: 'webRtcCollaborator', position: { x: COL * 3.2, y: 0 } },
			{ key: 'spk', kind: 'speaker', position: { x: COL * 3.2, y: ROW * 1.4 } }
		],
		edges: [
			{ from: 'mic', to: 'ns' },
			{ from: 'ns', to: 'comp' },
			{ from: 'comp', to: 'rtc' },
			{ from: 'comp', to: 'spk' }
		]
	}
];
