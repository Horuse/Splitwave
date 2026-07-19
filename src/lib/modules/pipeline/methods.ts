import { LazyStore } from '@tauri-apps/plugin-store';
import type { Pipeline } from './types';
import { PIPELINE_VERSION } from './version';

const STORE_FILE = 'pipelines.json';
const KEY_PREFIX = 'pipeline:';
const SNAPSHOT_KEY_PREFIX = 'snapshots:';
const MAX_SNAPSHOTS = 20;
const store = new LazyStore(STORE_FILE);

export interface Snapshot {
	takenAt: number;
	pipeline: Pipeline;
}

export const methods = {
	emptyPipeline(id: string, name: string): Pipeline {
		const now = Date.now();
		return {
			id,
			name,
			nodes: [],
			edges: [],
			createdAt: now,
			updatedAt: now,
			version: PIPELINE_VERSION
		};
	},

	async list(): Promise<Pipeline[]> {
		const entries = await store.entries<Pipeline>();
		return entries
			.filter(([k]) => k.startsWith(KEY_PREFIX))
			.map(([, v]) => v)
			.sort((a, b) => b.updatedAt - a.updatedAt);
	},

	async get(id: string): Promise<Pipeline | null> {
		return (await store.get<Pipeline>(KEY_PREFIX + id)) ?? null;
	},

	async save(p: Pipeline): Promise<void> {
		await store.set(KEY_PREFIX + p.id, { ...p, version: PIPELINE_VERSION });
		await store.save();
	},

	async remove(id: string): Promise<void> {
		await store.delete(KEY_PREFIX + id);
		await store.delete(SNAPSHOT_KEY_PREFIX + id);
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
		if (existing.length > MAX_SNAPSHOTS) {
			existing.splice(0, existing.length - MAX_SNAPSHOTS);
		}
		await store.set(key, existing);
		await store.save();
	}
};
