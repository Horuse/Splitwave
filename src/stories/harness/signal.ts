import { emit } from '@tauri-apps/api/event';

export type SignalShape = 'music' | 'sine' | 'noise' | 'impulses' | 'sweep' | 'silence' | 'off';

/** The synthetic stream a story feeds the node, as the engine would. */
export interface SignalOptions {
	signal: SignalShape;
	/** Peak level of the generated signal, dBFS. */
	signalLevel: number;
	signalFrequency: number;
	signalChannels: number;
	/** Gain reduction reported by dynamics nodes, dB. */
	gainReduction: number;
	/** Swing the level and gain reduction so meters move. */
	signalMotion: boolean;
	signalClipping: boolean;
}

export const DEFAULT_SIGNAL: SignalOptions = {
	signal: 'music',
	signalLevel: -6,
	signalFrequency: 440,
	signalChannels: 2,
	gainReduction: 4,
	signalMotion: true,
	signalClipping: false
};

export const SIGNAL_SHAPES: SignalShape[] = ['music', 'sine', 'noise', 'impulses', 'sweep', 'silence', 'off'];

const SR = 48_000;
const SCOPE_FRAMES = 1024;
const SPECTRUM_FRAMES = 4096;
const TICK_MS = 40;

const PARTIALS = [
	{ f: 110, a: 0.32 },
	{ f: 220, a: 0.18 },
	{ f: 440, a: 0.22 },
	{ f: 1200, a: 0.12 },
	{ f: 3500, a: 0.06 },
	{ f: 7800, a: 0.03 }
];

function dbToAmp(db: number): number {
	return Math.pow(10, db / 20);
}

function ampToDb(amp: number): number {
	return 20 * Math.log10(Math.max(amp, 1e-6));
}

function freqToBar(freq: number): number {
	const clamped = Math.max(F_MIN, Math.min(F_MAX, freq));
	return (Math.log(clamped / F_MIN) / LOG_RATIO) * BARS;
}

const BARS = 80;
const F_MIN = 20;
const F_MAX = SR / 2;
const LOG_RATIO = Math.log(F_MAX / F_MIN);

function spectrumBars(opts: SignalOptions, time: number): number[] {
	const DB_FLOOR = -96;
	if (opts.signal === 'off' || opts.signal === 'silence') {
		return new Array(BARS).fill(DB_FLOOR);
	}

	const env = opts.signalMotion ? 0.55 + 0.45 * Math.sin(2 * Math.PI * 0.12 * time) : 1;
	const peak = opts.signalLevel + 20 * Math.log10(Math.max(1e-4, env));
	const bars = new Array<number>(BARS);

	switch (opts.signal) {
		case 'sine': {
			const target = freqToBar(opts.signalFrequency);
			for (let b = 0; b < BARS; b++) {
				const d = Math.abs(b - target);
				const drop = Math.min(80, d * d * 18);
				const noise = Math.random() * 1.5 - 0.75;
				bars[b] = Math.max(DB_FLOOR, peak - drop + noise);
			}
			break;
		}
		case 'sweep': {
			const phase = (time % 4) / 4;
			const f = 20 * Math.pow(1000, phase);
			const target = freqToBar(f);
			for (let b = 0; b < BARS; b++) {
				const d = Math.abs(b - target);
				const drop = Math.min(80, d * d * 18);
				const noise = Math.random() * 1.5 - 0.75;
				bars[b] = Math.max(DB_FLOOR, peak - drop + noise);
			}
			break;
		}
		case 'noise': {
			for (let b = 0; b < BARS; b++) {
				const slope = (b / BARS) * 8;
				const noise = Math.random() * 3.5 - 1.75;
				bars[b] = Math.max(DB_FLOOR, peak - 8 - slope + noise);
			}
			break;
		}
		case 'impulses': {
			const pulse = (time * 4) % 1 < 0.04;
			for (let b = 0; b < BARS; b++) {
				const noise = Math.random() * 3 - 1.5;
				bars[b] = pulse ? Math.max(DB_FLOOR, peak - 6 + noise) : Math.max(DB_FLOOR, -85 + noise);
			}
			break;
		}
		default: {
			for (let b = 0; b < BARS; b++) {
				let maxLevel = -88 + (Math.random() * 2 - 1);
				for (const p of PARTIALS) {
					const target = freqToBar(p.f);
					const d = Math.abs(b - target);
					const drop = Math.min(75, d * d * 14);
					const lvl = peak + 20 * Math.log10(p.a) - drop;
					if (lvl > maxLevel) maxLevel = lvl;
				}
				bars[b] = Math.max(DB_FLOOR, Math.min(0, maxLevel));
			}
			break;
		}
	}

	if (opts.signalClipping) {
		for (let b = 0; b < BARS; b++) {
			bars[b] = Math.max(bars[b], peak - 14 - (b / BARS) * 8);
		}
	}

	return bars;
}

export interface SignalTargets {
	nodeId: string;
	/** Spectrum analyses a longer window than the scope. */
	wideScope: boolean;
	running: boolean;
}

/** Emits scope, meter, loudness, gain-reduction and transport ticks for one node. */
export function startSignal(targets: SignalTargets, opts: SignalOptions): () => void {
	if (opts.signal === 'off') return () => {};
	const { nodeId } = targets;
	const channels = Math.max(1, opts.signalChannels);
	let t = 0;
	let transportFrames = 0;
	const TOTAL_FRAMES = SR * 185;

	function envelope(time: number): number {
		return opts.signalMotion ? 0.55 + 0.45 * Math.sin(2 * Math.PI * 0.12 * time) : 1;
	}

	function sample(time: number, ch: number): number {
		const detune = 1 + ch * 0.003;
		switch (opts.signal) {
			case 'sine':
				return Math.sin(2 * Math.PI * opts.signalFrequency * detune * time);
			case 'noise':
				return Math.random() * 2 - 1;
			case 'impulses':
				return (time * 4) % 1 < 0.002 ? 1 : 0.01 * (Math.random() * 2 - 1);
			case 'sweep': {
				const phase = (time % 4) / 4;
				const f = 20 * Math.pow(1000, phase);
				return Math.sin(2 * Math.PI * f * time);
			}
			case 'silence':
				return 0;
			default: {
				let s = 0;
				for (const p of PARTIALS) s += p.a * Math.sin(2 * Math.PI * p.f * detune * time);
				return s + 0.02 * (Math.random() * 2 - 1);
			}
		}
	}

	function render(frames: number): number[][] {
		const peak = dbToAmp(opts.signalLevel);
		const out = Array.from({ length: channels }, () => new Array<number>(frames));
		for (let i = 0; i < frames; i++) {
			const time = t + i / SR;
			const env = envelope(time) * peak;
			for (let ch = 0; ch < channels; ch++) {
				const v = sample(time, ch) * env * (ch === 0 ? 1 : 0.85);
				out[ch][i] = opts.signalClipping ? Math.max(-1, Math.min(1, v * 4)) : v;
			}
		}
		return out;
	}

	function stats(block: number[][]) {
		return block.map((c) => {
			let peak = 0;
			let sum = 0;
			for (const v of c) {
				const a = Math.abs(v);
				if (a > peak) peak = a;
				sum += v * v;
			}
			return { peak, rms: Math.sqrt(sum / c.length) };
		});
	}

	const timer = setInterval(() => {
		const block = render(SCOPE_FRAMES);
		const scope = targets.wideScope ? render(SPECTRUM_FRAMES) : block;
		t += SCOPE_FRAMES / SR;
		const st = stats(block);

		emit('audio://scope', { nodeId, channels, data: scope, sampleRate: SR });
		if (targets.wideScope) {
			emit('audio://spectrum', { nodeId, bars: spectrumBars(opts, t), sampleRate: SR });
		}
		emit('audio://meter', { nodeId, peaks: st.map((s) => s.peak), rms: st.map((s) => s.rms) });

		const grDb = opts.gainReduction * (opts.signalMotion ? 0.5 + 0.5 * Math.abs(Math.sin(2 * Math.PI * 0.3 * t)) : 1);
		emit('audio://gr', { nodeId, grLin: dbToAmp(-grDb) });

		const peakDb = ampToDb(st[0].peak);
		const rmsDb = ampToDb(st[0].rms);
		const loud = opts.signal === 'silence' ? -70 : rmsDb - 0.7;
		emit('audio://lufs', {
			nodeId,
			momentary: loud,
			shortterm: loud - 0.8,
			integrated: loud - 1.4,
			tpL: peakDb,
			tpR: ampToDb(st[Math.min(1, st.length - 1)].peak),
			lra: opts.signalMotion ? 6.3 : 1.2,
			rms: rmsDb,
			noiseFloor: -68,
			samplePeak: peakDb,
			dcOffset: 0.0008,
			correlation: channels > 1 ? 0.86 : 1,
			clips: opts.signalClipping ? Math.floor(t * 3) : 0
		});

		if (targets.running) {
			transportFrames = (transportFrames + SCOPE_FRAMES) % TOTAL_FRAMES;
			emit('audio://audio_file_progress', {
				nodeId,
				frames: transportFrames,
				totalFrames: TOTAL_FRAMES,
				sampleRate: 44_100,
				channels,
				stopped: false,
				paused: false
			});
			emit('audio://recorder_progress', { nodeId, frames: Math.floor(t * SR), sampleRate: SR, stopped: false });
		}
	}, TICK_MS);

	return () => clearInterval(timer);
}
