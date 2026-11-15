import { browser } from '$app/environment';
import { getBezierPath, getSmoothStepPath, getStraightPath } from '@xyflow/svelte';

export type EdgeShape = 'bezier' | 'smoothstep' | 'step' | 'straight';

const KEY = 'flow:edge';

interface Stored {
	shape: EdgeShape;
	animated: boolean;
	pins: boolean;
}

const DEFAULTS: Stored = { shape: 'bezier', animated: true, pins: true };

function load(): Stored {
	if (!browser) return DEFAULTS;
	try {
		return { ...DEFAULTS, ...JSON.parse(window.localStorage.getItem(KEY) ?? '{}') };
	} catch {
		return DEFAULTS;
	}
}

/** `animated` drives xyflow's dashdraw, an infinite CSS animation per edge path. */
class EdgeSettings {
	#initial = load();
	shape = $state<EdgeShape>(this.#initial.shape);
	animated = $state(this.#initial.animated);
	pins = $state(this.#initial.pins);

	persist(): void {
		if (!browser) return;
		const { shape, animated, pins } = this;
		window.localStorage.setItem(KEY, JSON.stringify({ shape, animated, pins }));
	}

	reset(): void {
		this.shape = DEFAULTS.shape;
		this.animated = DEFAULTS.animated;
		this.pins = DEFAULTS.pins;
		this.persist();
	}
}

export const edgeSettings = new EdgeSettings();

type Route = Parameters<typeof getBezierPath>[0];

export function edgePath(p: Route): string {
	switch (edgeSettings.shape) {
		case 'straight':
			return getStraightPath(p)[0];
		case 'step':
			return getSmoothStepPath({ ...p, borderRadius: 0 })[0];
		case 'smoothstep':
			return getSmoothStepPath(p)[0];
		default:
			return getBezierPath(p)[0];
	}
}
