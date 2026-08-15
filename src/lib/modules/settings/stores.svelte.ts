import { browser } from '$app/environment';
import { isEnabled } from '@tauri-apps/plugin-autostart';

const KEY = 'app:settings';

interface Stored {
	checkUpdatesOnLaunch: boolean;
	maxSnapshots: number;
	snapToGrid: boolean;
	gridSize: number;
	launchOnStartup: boolean;
}

const DEFAULTS: Stored = {
	checkUpdatesOnLaunch: true,
	maxSnapshots: 20,
	snapToGrid: false,
	gridSize: 20,
	launchOnStartup: false
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
	launchOnStartup = $state(this.#initial.launchOnStartup);

	persist(): void {
		if (!browser) return;
		const { checkUpdatesOnLaunch, maxSnapshots, snapToGrid, gridSize, launchOnStartup } = this;
		window.localStorage.setItem(KEY, JSON.stringify({ checkUpdatesOnLaunch, maxSnapshots, snapToGrid, gridSize, launchOnStartup }));
	}

	reset(): void {
		Object.assign(this, DEFAULTS);
		this.persist();
	}

	/** Reconciles the mirror from the plugin's real OS registration state,
	 * in case the user removed Splitwave from login items outside the app. */
	async syncLaunchOnStartup(): Promise<void> {
		if (!browser) return;
		try {
			this.launchOnStartup = await isEnabled();
		} catch {
			// Plugin unavailable (e.g. non-Tauri dev preview) -- leave the mirror as-is.
		}
	}
}

export const appSettings = new AppSettings();
