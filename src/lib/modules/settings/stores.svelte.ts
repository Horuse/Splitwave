import { browser } from '$app/environment';
import { isEnabled } from '@tauri-apps/plugin-autostart';
import { getCurrentWebview } from '@tauri-apps/api/webview';

const KEY = 'app:settings';

interface Stored {
	checkUpdatesOnLaunch: boolean;
	includePreReleases: boolean;
	maxSnapshots: number;
	snapToGrid: boolean;
	gridSize: number;
	launchOnStartup: boolean;
	confirmOverwriteChanges: boolean;
	keepRunningOnDisconnect: boolean;
	pipelineSampleRate: number;
	uiScale: number;
}

const DEFAULTS: Stored = {
	checkUpdatesOnLaunch: true,
	includePreReleases: false,
	maxSnapshots: 20,
	snapToGrid: false,
	gridSize: 20,
	launchOnStartup: false,
	confirmOverwriteChanges: true,
	keepRunningOnDisconnect: true,
	pipelineSampleRate: 48_000,
	uiScale: 100
};

export const SNAPSHOT_LIMITS = [10, 20, 50, 100] as const;
export const GRID_SIZES = [10, 20, 40] as const;
export const UI_SCALE_MIN = 50;
export const UI_SCALE_MAX = 200;
export const UI_SCALE_STEP = 10;
export const PIPELINE_SAMPLE_RATE_PRESETS = [44100, 48000, 88200, 96000, 176400, 192000] as const;

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
	includePreReleases = $state(this.#initial.includePreReleases ?? false);
	maxSnapshots = $state(this.#initial.maxSnapshots);
	snapToGrid = $state(this.#initial.snapToGrid);
	gridSize = $state(this.#initial.gridSize);
	launchOnStartup = $state(this.#initial.launchOnStartup);
	confirmOverwriteChanges = $state(this.#initial.confirmOverwriteChanges);
	keepRunningOnDisconnect = $state(this.#initial.keepRunningOnDisconnect);
	pipelineSampleRate = $state(this.#initial.pipelineSampleRate ?? 48_000);
	uiScale = $state(this.#initial.uiScale ?? 100);

	persist(): void {
		if (!browser) return;
		const {
			checkUpdatesOnLaunch,
			includePreReleases,
			maxSnapshots,
			snapToGrid,
			gridSize,
			launchOnStartup,
			confirmOverwriteChanges,
			keepRunningOnDisconnect,
			pipelineSampleRate,
			uiScale
		} = this;
		window.localStorage.setItem(
			KEY,
			JSON.stringify({
				checkUpdatesOnLaunch,
				includePreReleases,
				maxSnapshots,
				snapToGrid,
				gridSize,
				launchOnStartup,
				confirmOverwriteChanges,
				keepRunningOnDisconnect,
				pipelineSampleRate,
				uiScale
			})
		);
	}

	reset(): void {
		Object.assign(this, DEFAULTS);
		this.persist();
		this.applyUiScale();
	}

	setUiScale(percent: number): void {
		this.uiScale = Math.min(Math.max(Math.round(percent), UI_SCALE_MIN), UI_SCALE_MAX);
		this.persist();
		this.applyUiScale();
	}

	applyUiScale(): void {
		if (!browser) return;
		document.documentElement.style.setProperty('--ui-zoom', String(this.uiScale / 100));
		getCurrentWebview()
			.setZoom(this.uiScale / 100)
			.catch(() => {});
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
