// Per-format recording capability config for the File Recording node:
// extension, channel cap, rate grid and bitrate presets/bounds. Kept in sync
// with the backend validations (graph.rs, resolve_output). Probed facts baked
// into these numbers:
//   • Apple's AAC encoder encodes mono/stereo only, at 32/44.1/48
//     kHz, with bitrate bounds scaling by channel count under a 320 kbps
//     absolute cap (lower rates fail at file creation, higher ones at first
//     write).
//   • FLAC's format spans 8..655350 Hz (20-bit STREAMINFO rate field).
//   • LAME CBR ranges track the MPEG layer of the sample rate.
//   • Opus is the libopus 6..510 kbps range.

interface RateConfig {
	// fixed: the backend locks the rate (48 kHz), no selector rendered.
	// grid: a fixed grid only (no Custom). grid+custom: grid + a numeric
	// Custom input clamped to min..max.
	mode: 'fixed' | 'grid' | 'grid+custom';
	rates?: number[];
	columns?: number;
	min?: number;
	max?: number;
}

interface BitrateConfig {
	// All bitrate numbers are kbps; `storedUnit` is what the node's format
	// field stores (mp3: kbps, aac/opus: bps).
	storedUnit: 'kbps' | 'bps';
	// Popularity-ordered kbps presets shown in the grid, the full kbps ladder
	// for the fallback sampling, the stepper's kbps step, and the encoder's
	// [min, max] in kbps (`perChannel` multiplies them by the channel count,
	// capped at `absoluteMax`).
	presets: number[];
	ladder: number[];
	step: number;
	boundsByRate: Record<string, { min: number; max: number }>;
	perChannel?: boolean;
	absoluteMax?: number;
}

import type { RecordingFormat } from './types';

export interface FormatConfig {
	extension: string;
	maxChannels: number;
	rate: RateConfig;
	bitrate: BitrateConfig | null;
}

export const RECORDING_FORMATS: Record<RecordingFormat['kind'], FormatConfig> = {
	wav: {
		extension: 'wav',
		maxChannels: 512,
		rate: { mode: 'grid+custom', rates: [44_100, 48_000, 88_200, 96_000], columns: 5, min: 8_000, max: 384_000 },
		bitrate: null
	},
	aiff: {
		extension: 'aiff',
		maxChannels: 512,
		rate: { mode: 'grid+custom', rates: [44_100, 48_000, 88_200, 96_000], columns: 5, min: 8_000, max: 384_000 },
		bitrate: null
	},
	flac: {
		extension: 'flac',
		maxChannels: 8,
		rate: {
			mode: 'grid+custom',
			rates: [44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 256_000, 352_800, 384_000],
			columns: 5,
			min: 8_000,
			max: 655_350
		},
		bitrate: null
	},
	aac: {
		extension: 'm4a',
		maxChannels: 2,
		rate: { mode: 'grid', rates: [32_000, 44_100, 48_000], columns: 3 },
		bitrate: {
			storedUnit: 'bps',
			presets: [96, 128, 192, 256],
			ladder: [24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320],
			step: 8,
			boundsByRate: {
				'32000': { min: 24, max: 96 },
				default: { min: 32, max: 256 }
			},
			perChannel: true,
			absoluteMax: 320
		}
	},
	mp3: {
		extension: 'mp3',
		maxChannels: 2,
		rate: { mode: 'fixed' },
		bitrate: {
			storedUnit: 'kbps',
			presets: [128, 192, 256, 320],
			ladder: [8, 16, 24, 32, 40, 48, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320],
			step: 16,
			boundsByRate: {
				'32000': { min: 32, max: 320 },
				'44100': { min: 32, max: 320 },
				'48000': { min: 32, max: 320 },
				'16000': { min: 8, max: 160 },
				'22050': { min: 8, max: 160 },
				'24000': { min: 8, max: 160 },
				'8000': { min: 8, max: 64 },
				'11025': { min: 8, max: 64 },
				'12000': { min: 8, max: 64 },
				// mp3 pins the rate, so node data may carry no sampleRate at all;
				// the default covers MPEG-1 rates (32..320).
				default: { min: 32, max: 320 }
			},
			perChannel: false
		}
	},
	opus: {
		extension: 'opus',
		maxChannels: 2,
		rate: { mode: 'fixed' },
		bitrate: {
			storedUnit: 'bps',
			presets: [64, 96, 128, 160, 192, 256],
			ladder: [6, 16, 24, 32, 48, 64, 96, 128, 192, 256, 320, 448, 510],
			step: 16,
			boundsByRate: {
				default: { min: 6, max: 510 }
			},
			perChannel: false
		}
	}
} satisfies Record<string, FormatConfig>;
