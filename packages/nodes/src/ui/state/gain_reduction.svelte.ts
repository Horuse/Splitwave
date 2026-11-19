import { onDestroy, onMount } from 'svelte';
import { getEnginePort } from '../../engine';

const FALL_DB_PER_SEC = 18;

export interface GainReduction {
	/** Smoothed reduction in dB, >= 0: jumps up at once, falls back slowly so a peak stays readable. */
	readonly db: number;
	/** Last raw gain the engine reported, 0..1; 1 means untouched. */
	readonly gain: number;
}

/** Follows `audio://gr` for one node. Call during component init. */
export function useGainReduction(nodeId: () => string): GainReduction {
	const engine = getEnginePort();
	let target = 0;
	let gain = $state(1);
	let db = $state(0);
	let raf: number | undefined;
	let last = 0;

	engine.listen<{ nodeId: string; grLin: number }>('audio://gr', (p) => {
		if (p.nodeId !== nodeId()) return;
		gain = Math.max(0, Math.min(1, p.grLin));
		target = -20 * Math.log10(Math.max(1e-6, gain));
	});

	function tick(now: number) {
		const dt = last ? Math.min((now - last) / 1000, 0.1) : 0;
		last = now;
		db = target > db ? target : Math.max(target, db - FALL_DB_PER_SEC * dt);
		raf = requestAnimationFrame(tick);
	}

	onMount(() => {
		raf = requestAnimationFrame(tick);
	});
	onDestroy(() => {
		if (raf !== undefined) cancelAnimationFrame(raf);
	});

	return {
		get db() {
			return db;
		},
		get gain() {
			return gain;
		}
	};
}
