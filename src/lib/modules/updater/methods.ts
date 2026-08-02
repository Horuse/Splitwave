import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { LazyStore } from '@tauri-apps/plugin-store';
import { invoke } from '@tauri-apps/api/core';
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
		updaterStore.state = {
			phase: 'available',
			update,
			notes: (await releaseNotes(update.version)) ?? update.body ?? null
		};
	} catch (e) {
		const message = await diagnoseError(e);
		updaterStore.state = silent ? { phase: 'idle' } : { phase: 'error', message };
	}
}

const RELEASES_API = 'https://api.github.com/repos/Horuse/Splitwave/releases';

export interface Release {
	version: string;
	notes: string | null;
}

/** `null` when the release is unreachable. */
async function fetchRelease(path: string): Promise<Release | null> {
	try {
		const res = await fetch(RELEASES_API + path, {
			headers: { Accept: 'application/vnd.github+json' }
		});
		if (!res.ok) return null;
		const json = await res.json();
		const body = typeof json.body === 'string' ? json.body.trim() : '';
		const tag = typeof json.tag_name === 'string' ? json.tag_name : '';
		return { version: tag.replace(/^v/, ''), notes: body || null };
	} catch {
		return null;
	}
}

/** The release description on GitHub, so `latest.json` doesn't have to carry
 * the changelog. */
async function releaseNotes(version: string): Promise<string | null> {
	return (await fetchRelease(`/tags/v${version}`))?.notes ?? null;
}

export function latestRelease(): Promise<Release | null> {
	return fetchRelease('/latest');
}

async function diagnoseError(e: unknown): Promise<string> {
	const base = e instanceof Error ? e.message : String(e);
	try {
		const detail = await invoke<string>('diagnose_update_error');
		return detail ? `${base}\n\n${detail}` : base;
	} catch {
		return base;
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
