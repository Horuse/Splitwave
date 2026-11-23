import { onDestroy } from 'svelte';
import { getEnginePort } from '../../engine';

const FALL_DB_PER_SEC = 18;

export interface GainReduction {
	/** Smoothed reduction in dB, >= 0: jumps up at once, falls back slowly so a peak stays readable. */
	readonly db: number;
	/** Last raw gain the engine reported, 0..1; 1 means untouched. */
	readonly gain: number;
	/** Detector level feeding the effect, dBFS, with the same peak fall as `db`; null while silent. */
	readonly inDb: number | null;
	/** Last detector level the engine reported, dBFS, without the fall: what the effect compares to its threshold. */
	readonly inRawDb: number;
}

const IN_FLOOR_DB = -90;

/** Follows `audio://gr` for one node. Call during component init. */
export function useGainReduction(nodeId: () => string): GainReduction {
	const engine = getEnginePort();
	let target = 0;
	let inTarget = IN_FLOOR_DB;
	let gain = $state(1);
	let db = $state(0);
	let inLevel = $state(IN_FLOOR_DB);
	let inRaw = $state(IN_FLOOR_DB);
	let raf: number | undefined;
	let destroyed = false;
	let last = 0;

	engine.listen<{ nodeId: string; grLin: number; inLin?: number }>('audio://gr', (p) => {
		if (p.nodeId !== nodeId()) return;
		gain = Math.max(0, Math.min(1, p.grLin));
		target = -20 * Math.log10(Math.max(1e-6, gain));
		if (p.inLin !== undefined) {
			inTarget = Math.max(IN_FLOOR_DB, 20 * Math.log10(Math.max(1e-6, p.inLin)));
			inRaw = inTarget;
		}
		wake();
	});

	// Frames run only while a reading is still falling toward its target, so an
	// idle node costs nothing between engine ticks.
	function wake() {
		if (raf !== undefined || destroyed) return;
		last = 0;
		raf = requestAnimationFrame(tick);
	}

	function tick(now: number) {
		const dt = last ? Math.min((now - last) / 1000, 0.1) : 0;
		last = now;
		db = target > db ? target : Math.max(target, db - FALL_DB_PER_SEC * dt);
		inLevel = inTarget > inLevel ? inTarget : Math.max(inTarget, inLevel - FALL_DB_PER_SEC * dt);
		raf = db === target && inLevel === inTarget ? undefined : requestAnimationFrame(tick);
	}

	onDestroy(() => {
		destroyed = true;
		if (raf !== undefined) cancelAnimationFrame(raf);
		raf = undefined;
	});

	return {
		get db() {
			return db;
		},
		get gain() {
			return gain;
		},
		get inRawDb() {
			return inRaw;
		},
		get inDb() {
			return inLevel > IN_FLOOR_DB + 1 ? inLevel : null;
		}
	};
}
