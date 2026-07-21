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

// Compressor thresholds assume roughly -18 to -12 dBFS in; trim to taste, the
// target is the gain reduction (3-6 dB for natural speech), not the number.
export const FACTORY_PRESETS: Preset[] = [
	make('compressor', 'Spoken Voice / Podcast', {
		thresholdDb: -18,
		ratio: 3,
		attackMs: 8,
		releaseMs: 100,
		kneeDb: 6,
		makeupDb: 4
	}),
	make('compressor', 'Broadcast Voice Leveling', {
		thresholdDb: -24,
		ratio: 4,
		attackMs: 5,
		releaseMs: 80,
		kneeDb: 6,
		makeupDb: 6
	}),
	make('compressor', 'Dynamic Voice Tamer', {
		thresholdDb: -24,
		ratio: 8,
		attackMs: 3,
		releaseMs: 100,
		kneeDb: 3,
		makeupDb: 6
	}),
	make('compressor', 'Lead Vocal (Singing)', {
		thresholdDb: -20,
		ratio: 3,
		attackMs: 15,
		releaseMs: 50,
		kneeDb: 6,
		makeupDb: 4
	}),
	make('compressor', 'Drum Bus Punch', {
		thresholdDb: -18,
		ratio: 4,
		attackMs: 20,
		releaseMs: 100,
		kneeDb: 0,
		makeupDb: 4
	}),
	make('compressor', 'Mastering Glue', {
		thresholdDb: -12,
		ratio: 2,
		attackMs: 30,
		releaseMs: 300,
		kneeDb: 12,
		makeupDb: 2
	}),

	make('limiter', 'Streaming Master (-1 dBTP)', {
		ceilingDb: -1,
		lookaheadMs: 5,
		releaseMs: 100
	}),
	make('limiter', 'Podcast Safety', { ceilingDb: -1, lookaheadMs: 5, releaseMs: 150 }),
	make('limiter', 'ACX Peak Guard (-3 dB)', { ceilingDb: -3, lookaheadMs: 5, releaseMs: 100 }),
	make('limiter', 'Broadcast (EBU -1 dBTP)', { ceilingDb: -1, lookaheadMs: 10, releaseMs: 200 }),
	// Alexa and Echo playback clips between samples, hence the extra decibel.
	make('limiter', 'Lossy-Safe / Amazon (-2 dBTP)', {
		ceilingDb: -2,
		lookaheadMs: 5,
		releaseMs: 100
	}),
	make('limiter', 'Loud / Club', { ceilingDb: -0.3, lookaheadMs: 1, releaseMs: 50 }),

	make('noiseGate', 'Voice / Podcast', {
		thresholdDb: -45,
		rangeDb: -20,
		attackMs: 2,
		holdMs: 25,
		releaseMs: 150
	}),
	make('noiseGate', 'Dialogue (Subtle)', {
		thresholdDb: -55,
		rangeDb: -12,
		attackMs: 5,
		holdMs: 100,
		releaseMs: 300
	}),
	make('noiseGate', 'Snare / Drum Isolation', {
		thresholdDb: -18,
		rangeDb: -80,
		attackMs: 0.5,
		holdMs: 20,
		releaseMs: 100
	}),
	make('noiseGate', 'Kick Drum', {
		thresholdDb: -20,
		rangeDb: -80,
		attackMs: 1,
		holdMs: 30,
		releaseMs: 150
	}),
	make('noiseGate', 'Guitar Amp Hum Gate', {
		thresholdDb: -50,
		rangeDb: -60,
		attackMs: 1,
		holdMs: 50,
		releaseMs: 200
	}),

	// Fixed ISO octave bands, so a "high-pass at 80 Hz" can only be approximated
	// by pulling the 32 and 64 Hz bands down hard.
	make('eq', 'Spoken Voice Cleanup', { gainsDb: [-18, -9, 0, -2, -1, 0, 3, 2, 1, 0] }),
	make('eq', 'Podcast Warmth (Male)', { gainsDb: [-18, -6, 2, 0, -1, 0, 2, 3, 1, -2] }),
	make('eq', 'Clarity / De-Mud', { gainsDb: [-12, -6, -2, -3, -2, 0, 2, 3, 2, 0] }),
	make('eq', 'Air / Bright Vocal', { gainsDb: [-12, -6, 0, -1, 0, 0, 1, 2, 3, 3] }),
	make('eq', 'Telephone / Lo-Fi FX', { gainsDb: [-18, -18, -12, 0, 3, 3, 2, 0, -18, -18] }),
	make('eq', 'Playback Smile Curve', { gainsDb: [4, 3, 1, 0, -1, -1, 0, 1, 3, 4] }),

	make('saturator', 'Subtle Warmth', { thresholdDb: -6, driveDb: 3 }),
	make('saturator', 'Vocal Presence', { thresholdDb: -6, driveDb: 6 }),
	make('saturator', 'Tape Drive', { thresholdDb: -3, driveDb: 10 }),
	make('saturator', 'Bass Thickener', { thresholdDb: -6, driveDb: 8 }),
	make('saturator', 'Aggressive Color', { thresholdDb: -2, driveDb: 15 }),

	// Tempo-synced times are 60000 / BPM at 120 BPM.
	make('delay', 'Slapback Vocal', { timeMs: 90, feedback: 0.1, mix: 0.25 }),
	make('delay', 'Rockabilly Slap (Guitar)', { timeMs: 80, feedback: 0.05, mix: 0.35 }),
	make('delay', '1/4 Note @ 120bpm', { timeMs: 500, feedback: 0.3, mix: 0.22 }),
	make('delay', '1/8 Note @ 120bpm', { timeMs: 250, feedback: 0.35, mix: 0.2 }),
	make('delay', 'Dotted 1/8 @ 120bpm', { timeMs: 375, feedback: 0.3, mix: 0.2 }),
	make('delay', 'Ambient / Dub', { timeMs: 400, feedback: 0.6, mix: 0.3 }),

	make('reverb', 'Vocal Plate', { roomSize: 0.5, damping: 0.4, width: 1, mix: 0.2 }),
	make('reverb', 'Small Room (Drums)', { roomSize: 0.3, damping: 0.5, width: 0.8, mix: 0.15 }),
	make('reverb', 'Hall / Ballad', { roomSize: 0.85, damping: 0.3, width: 1, mix: 0.25 }),
	make('reverb', 'Subtle Ambience', { roomSize: 0.4, damping: 0.6, width: 0.7, mix: 0.12 }),
	make('reverb', 'Large / Cinematic', { roomSize: 0.95, damping: 0.2, width: 1, mix: 0.3 }),

	// DeepFilterNet is a speech model: on music it guards vocal bands and eats
	// the instruments. Values track the EasyEffects LADSPA defaults.
	make('noiseSuppressor', 'Factory Default', {
		attenuationLimitDb: 100,
		postFilterBeta: 0.02,
		minThreshDb: -10,
		maxErbThreshDb: 30,
		maxDfThreshDb: 20
	}),
	make('noiseSuppressor', 'Balanced Voice', {
		attenuationLimitDb: 75,
		postFilterBeta: 0.02,
		minThreshDb: -10,
		maxErbThreshDb: 30,
		maxDfThreshDb: 20
	}),
	make('noiseSuppressor', 'Gentle / Natural', {
		attenuationLimitDb: 40,
		postFilterBeta: 0.02,
		minThreshDb: -15,
		maxErbThreshDb: 30,
		maxDfThreshDb: 20
	}),
	make('noiseSuppressor', 'Aggressive (Noisy Room)', {
		attenuationLimitDb: 100,
		postFilterBeta: 0.05,
		minThreshDb: -5,
		maxErbThreshDb: 20,
		maxDfThreshDb: 20
	}),
	make('noiseSuppressor', 'Broadcast Light', {
		attenuationLimitDb: 60,
		postFilterBeta: 0.02,
		minThreshDb: -12,
		maxErbThreshDb: 30,
		maxDfThreshDb: 20
	}),

	// Frequency is where the sibilant band starts; drop threshold until the "s"
	// stops spiking. Female/bright voices sibilate higher than male voices.
	make('deEsser', 'Voice / Podcast', { frequency: 6500, thresholdDb: -35, ratio: 4 }),
	make('deEsser', 'Gentle', { frequency: 7000, thresholdDb: -30, ratio: 3 }),
	make('deEsser', 'Male Voice', { frequency: 6000, thresholdDb: -34, ratio: 4 }),
	make('deEsser', 'Female / Bright', { frequency: 7500, thresholdDb: -38, ratio: 5 }),
	make('deEsser', 'Aggressive', { frequency: 6000, thresholdDb: -45, ratio: 8 })
];
