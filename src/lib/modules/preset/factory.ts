import type { Preset, PresetKind } from './types';
import { PRESET_VERSION } from './version';

function make<K extends PresetKind>(kind: K, name: string, data: Preset<K>['data']): Preset<K> {
	return {
		id: `builtin:${kind}:${name}`,
		kind,
		name,
		data,
		createdAt: 0,
		builtin: true,
		version: PRESET_VERSION
	};
}

// EQ bands are the ten ISO octave centres: 31.5 63 125 250 500 1k 2k 4k 8k 16k.
export const FACTORY_PRESETS: Preset[] = [
	make('compressor', 'Vocal', {
		thresholdDb: -18,
		ratio: 3,
		attackMs: 8,
		releaseMs: 120,
		kneeDb: 6,
		makeupDb: 3
	}),
	make('compressor', 'Podcast Leveler', {
		thresholdDb: -24,
		ratio: 4,
		attackMs: 15,
		releaseMs: 200,
		kneeDb: 8,
		makeupDb: 5
	}),
	make('compressor', 'Drum Bus', {
		thresholdDb: -12,
		ratio: 4,
		attackMs: 3,
		releaseMs: 80,
		kneeDb: 3,
		makeupDb: 2
	}),
	make('compressor', 'Master Glue', {
		thresholdDb: -10,
		ratio: 2,
		attackMs: 30,
		releaseMs: 250,
		kneeDb: 9,
		makeupDb: 1
	}),
	make('compressor', 'Peak Tamer', {
		thresholdDb: -6,
		ratio: 10,
		attackMs: 1,
		releaseMs: 50,
		kneeDb: 0,
		makeupDb: 0
	}),

	make('noiseGate', 'Vocal Cleanup', {
		thresholdDb: -45,
		rangeDb: -30,
		attackMs: 1,
		holdMs: 60,
		releaseMs: 120
	}),
	make('noiseGate', 'Room Tone Kill', {
		thresholdDb: -55,
		rangeDb: -20,
		attackMs: 5,
		holdMs: 150,
		releaseMs: 250
	}),
	make('noiseGate', 'Drum Tight', {
		thresholdDb: -30,
		rangeDb: -40,
		attackMs: 0.5,
		holdMs: 30,
		releaseMs: 60
	}),

	make('limiter', 'Streaming -1 dB', { ceilingDb: -1, lookaheadMs: 5, releaseMs: 100 }),
	make('limiter', 'Broadcast -2 dB', { ceilingDb: -2, lookaheadMs: 5, releaseMs: 150 }),
	make('limiter', 'Master -0.3 dB', { ceilingDb: -0.3, lookaheadMs: 3, releaseMs: 60 }),
	make('limiter', 'Safety Catch', { ceilingDb: -0.1, lookaheadMs: 1, releaseMs: 30 }),

	make('eq', 'Flat', { gainsDb: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] }),
	make('eq', 'Bass Boost', { gainsDb: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0] }),
	make('eq', 'Treble Boost', { gainsDb: [0, 0, 0, 0, 0, 0, 2, 4, 5, 6] }),
	make('eq', 'Vocal', { gainsDb: [-4, -3, -1, 1, 3, 4, 4, 2, 0, -2] }),
	make('eq', 'Podcast', { gainsDb: [-6, -4, -2, 0, 2, 3, 4, 3, 1, -1] }),
	make('eq', 'Rock', { gainsDb: [4, 3, 1, -1, -1, 1, 3, 4, 5, 5] }),
	make('eq', 'Pop', { gainsDb: [-1, 0, 1, 2, 3, 2, 0, -1, -1, -2] }),
	make('eq', 'Jazz', { gainsDb: [3, 2, 1, 1, -1, -1, 0, 1, 2, 3] }),
	make('eq', 'Classical', { gainsDb: [4, 3, 2, 0, 0, 0, -1, -1, -2, -3] }),
	make('eq', 'Electronic', { gainsDb: [4, 3, 1, 0, -2, 1, 0, 1, 3, 4] }),

	make('reverb', 'Small Room', { roomSize: 0.3, damping: 0.6, width: 0.8, mix: 0.15 }),
	make('reverb', 'Vocal Plate', { roomSize: 0.55, damping: 0.4, width: 1, mix: 0.22 }),
	make('reverb', 'Large Hall', { roomSize: 0.85, damping: 0.3, width: 1, mix: 0.3 }),
	make('reverb', 'Subtle Space', { roomSize: 0.4, damping: 0.5, width: 0.7, mix: 0.1 }),

	make('delay', 'Slapback', { timeMs: 110, feedback: 0.15, mix: 0.25 }),
	make('delay', 'Eighth @ 120', { timeMs: 250, feedback: 0.35, mix: 0.3 }),
	make('delay', 'Quarter @ 120', { timeMs: 500, feedback: 0.4, mix: 0.28 }),
	make('delay', 'Ambient Wash', { timeMs: 700, feedback: 0.6, mix: 0.45 }),

	make('saturator', 'Warm Glue', { thresholdDb: -12, driveDb: 3 }),
	make('saturator', 'Tape Drive', { thresholdDb: -18, driveDb: 8 }),
	make('saturator', 'Hard Colour', { thresholdDb: -24, driveDb: 14 }),

	make('noiseSuppressor', 'Speech Light', {
		attenuationLimitDb: 12,
		postFilterBeta: 0,
		minThreshDb: -10,
		maxErbThreshDb: 30,
		maxDfThreshDb: 20
	}),
	make('noiseSuppressor', 'Speech Strong', {
		attenuationLimitDb: 40,
		postFilterBeta: 0.02,
		minThreshDb: -5,
		maxErbThreshDb: 25,
		maxDfThreshDb: 15
	}),
	make('noiseSuppressor', 'Max Suppression', {
		attenuationLimitDb: 100,
		postFilterBeta: 0.05,
		minThreshDb: 0,
		maxErbThreshDb: 20,
		maxDfThreshDb: 10
	})
];
