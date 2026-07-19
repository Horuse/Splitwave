import { browser } from '$app/environment';

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
