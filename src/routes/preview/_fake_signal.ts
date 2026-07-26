import { emit } from '@tauri-apps/api/event';

const SR = 48_000;
const SCOPE_FRAMES = 1024;
const SPECTRUM_FRAMES = 4096; // the engine's spectrum window size
const TICK_MS = 40;

const PARTIALS = [
	{ f: 110, a: 0.32 },
	{ f: 220, a: 0.18 },
	{ f: 440, a: 0.22 },
	{ f: 1200, a: 0.12 },
	{ f: 3500, a: 0.06 },
	{ f: 7800, a: 0.03 }
];

interface MonitorIds {
	levelMeter: string;
	lufsMeter: string;
	waveform: string;
	spectrum: string;
}

/** Synthetic 2-channel signal so the monitor nodes render live in the preview. */
export function startFakeSignal(ids: MonitorIds): () => void {
	const phase = PARTIALS.map(() => 0);
	let t = 0;

	function render(frames: number): number[][] {
		const left = new Array<number>(frames);
		const right = new Array<number>(frames);
		for (let i = 0; i < frames; i++) {
			// Slow swell keeps the meters moving instead of sitting on one value.
			const env = 0.55 + 0.4 * Math.sin(2 * Math.PI * 0.12 * (t + i / SR));
			let s = 0;
			for (let p = 0; p < PARTIALS.length; p++) {
				s += PARTIALS[p].a * Math.sin(phase[p] + (2 * Math.PI * PARTIALS[p].f * i) / SR);
			}
			s = (s + 0.02 * (Math.random() * 2 - 1)) * env;
			left[i] = s;
			right[i] = s * 0.85 + 0.03 * (Math.random() * 2 - 1);
		}
		return [left, right];
	}

	function advance(frames: number) {
		for (let p = 0; p < PARTIALS.length; p++) {
			phase[p] = (phase[p] + (2 * Math.PI * PARTIALS[p].f * frames) / SR) % (2 * Math.PI);
		}
		t += frames / SR;
	}

	function stats(chans: number[][]) {
		return chans.map((c) => {
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
		advance(SCOPE_FRAMES);
		const st = stats(block);

		emit('audio://scope', { nodeId: ids.waveform, channels: 2, data: block, sampleRate: SR });
		emit('audio://scope', {
			nodeId: ids.spectrum,
			channels: 2,
			data: render(SPECTRUM_FRAMES),
			sampleRate: SR
		});
		emit('audio://meter', {
			nodeId: ids.levelMeter,
			peaks: st.map((s) => s.peak),
			rms: st.map((s) => s.rms)
		});

		const lufs = -18 + 4 * Math.sin(2 * Math.PI * 0.08 * t);
		const tp = 20 * Math.log10(Math.max(st[0].peak, 1e-6));
		emit('audio://lufs', {
			nodeId: ids.lufsMeter,
			momentary: lufs,
			shortterm: lufs - 0.8,
			integrated: -17.4,
			tpL: tp,
			tpR: tp - 1.2,
			lra: 6.3,
			rms: 20 * Math.log10(Math.max(st[0].rms, 1e-6)),
			noiseFloor: -68,
			samplePeak: tp,
			dcOffset: 0.0008,
			correlation: 0.86,
			clips: 0
		});
	}, TICK_MS);

	return () => clearInterval(timer);
}
