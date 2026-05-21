import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { LazyStore } from '@tauri-apps/plugin-store';
import { updaterStore } from './stores.svelte';

const PREFS_FILE = 'updater_prefs.json';
const SKIPPED_KEY = 'skippedVersion';
const prefs = new LazyStore(PREFS_FILE);

export async function getSkippedVersion(): Promise<string | null> {
	try {
		return ((await prefs.get<string>(SKIPPED_KEY)) ?? null);
	} catch {
		return null;
	}
}

export async function skipVersion(version: string): Promise<void> {
	try {
		await prefs.set(SKIPPED_KEY, version);
		await prefs.save();
	} catch {
	}
	updaterStore.state = { phase: 'idle' };
}

export async function checkForUpdates(silent = false): Promise<void> {
	updaterStore.state = { phase: 'checking' };
	try {
		const update = await check();
		if (!update) {
			updaterStore.state = silent ? { phase: 'idle' } : { phase: 'up_to_date' };
			return;
		}
		// Manual menu check always surfaces the update; only the silent startup check honors a skip.
		if (silent && (await getSkippedVersion()) === update.version) {
			updaterStore.state = { phase: 'idle' };
			return;
		}
		updaterStore.state = { phase: 'available', update };
	} catch (e) {
		updaterStore.state = silent
			? { phase: 'idle' }
			: { phase: 'error', message: e instanceof Error ? e.message : String(e) };
	}
}

export async function installUpdate(): Promise<void> {
	const s = updaterStore.state;
	if (s.phase !== 'available') return;
	const update = s.update;

	updaterStore.state = { phase: 'downloading', update, downloaded: 0, total: null };
	try {
		await update.downloadAndInstall((event) => {
			if (event.event === 'Started') {
				updaterStore.state = {
					phase: 'downloading',
					update,
					downloaded: 0,
					total: event.data.contentLength ?? null
				};
			} else if (event.event === 'Progress') {
				const cur = updaterStore.state;
				if (cur.phase !== 'downloading') return;
				updaterStore.state = {
					phase: 'downloading',
					update,
					downloaded: cur.downloaded + event.data.chunkLength,
					total: cur.total
				};
			} else if (event.event === 'Finished') {
				updaterStore.state = { phase: 'installing', update };
			}
		});
		await relaunch();
	} catch (e) {
		updaterStore.state = { phase: 'error', message: e instanceof Error ? e.message : String(e) };
	}
}
