import { LazyStore } from '@tauri-apps/plugin-store';
import type { Pipeline } from './types';
import { PIPELINE_VERSION } from './version';
import { isFromFuture, migrate } from './migrations';
import { pruneDanglingEdges } from './sanitize';
import { appSettings } from '$lib/modules/settings/stores.svelte';

const STORE_FILE = 'pipelines.json';
const KEY_PREFIX = 'pipeline:';
const SNAPSHOT_KEY_PREFIX = 'snapshots:';
const ACTIVE_PIPELINE_KEY = 'activePipelineId';
const store = new LazyStore(STORE_FILE);

export interface Snapshot {
	takenAt: number;
	pipeline: Pipeline;
}

export const methods = {
	async list(): Promise<Pipeline[]> {
		const entries = await store.entries<Pipeline>();
		return entries
			.filter(([k]) => k.startsWith(KEY_PREFIX))
			.map(([, v]) => v)
			.sort((a, b) => b.updatedAt - a.updatedAt);
	},

	/** Migrates on read; the result is only persisted once the pipeline is saved,
	 * so opening a v0 pipeline in a build that crashes leaves the original intact. */
	async get(id: string): Promise<Pipeline | null> {
		const stored = await store.get<Pipeline>(KEY_PREFIX + id);
		if (!stored) return null;
		return isFromFuture(stored) ? stored : pruneDanglingEdges(migrate(stored));
	},

	async save(p: Pipeline): Promise<void> {
		const clean = pruneDanglingEdges(p);
		await store.set(KEY_PREFIX + p.id, { ...clean, version: PIPELINE_VERSION });
		await store.save();
	},

	async remove(id: string): Promise<void> {
		await store.delete(KEY_PREFIX + id);
		await store.delete(SNAPSHOT_KEY_PREFIX + id);
		await store.save();
	},

	async getActivePipelineId(): Promise<string | null> {
		return (await store.get<string>(ACTIVE_PIPELINE_KEY)) ?? null;
	},

	async setActivePipelineId(id: string | null): Promise<void> {
		if (id === null) {
			await store.delete(ACTIVE_PIPELINE_KEY);
		} else {
			await store.set(ACTIVE_PIPELINE_KEY, id);
		}
		await store.save();
	},

	async listSnapshots(id: string): Promise<Snapshot[]> {
		return (await store.get<Snapshot[]>(SNAPSHOT_KEY_PREFIX + id)) ?? [];
	},

	async addSnapshot(p: Pipeline): Promise<void> {
		const key = SNAPSHOT_KEY_PREFIX + p.id;
		const existing = (await store.get<Snapshot[]>(key)) ?? [];
		existing.push({ takenAt: Date.now(), pipeline: p });
		// Ring-buffer behaviour -- drop oldest.
		const cap = appSettings.maxSnapshots;
		if (existing.length > cap) {
			existing.splice(0, existing.length - cap);
		}
		await store.set(key, existing);
		await store.save();
	}
};
