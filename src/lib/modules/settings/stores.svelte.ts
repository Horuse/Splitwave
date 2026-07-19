import { browser } from '$app/environment';

const KEY = 'app:settings';

interface Stored {
	checkUpdatesOnLaunch: boolean;
	maxSnapshots: number;
	snapToGrid: boolean;
	gridSize: number;
}

const DEFAULTS: Stored = {
	checkUpdatesOnLaunch: true,
	maxSnapshots: 20,
	snapToGrid: false,
	gridSize: 20
};

export const SNAPSHOT_LIMITS = [10, 20, 50, 100] as const;
export const GRID_SIZES = [10, 20, 40] as const;

function load(): Stored {
	if (!browser) return DEFAULTS;
	try {
		return { ...DEFAULTS, ...JSON.parse(window.localStorage.getItem(KEY) ?? '{}') };
	} catch {
		return DEFAULTS;
	}
}

class AppSettings {
	#initial = load();
	checkUpdatesOnLaunch = $state(this.#initial.checkUpdatesOnLaunch);
	maxSnapshots = $state(this.#initial.maxSnapshots);
	snapToGrid = $state(this.#initial.snapToGrid);
	gridSize = $state(this.#initial.gridSize);

	persist(): void {
		if (!browser) return;
		const { checkUpdatesOnLaunch, maxSnapshots, snapToGrid, gridSize } = this;
		window.localStorage.setItem(
			KEY,
			JSON.stringify({ checkUpdatesOnLaunch, maxSnapshots, snapToGrid, gridSize })
		);
	}

	reset(): void {
		Object.assign(this, DEFAULTS);
		this.persist();
	}
}

export const appSettings = new AppSettings();
