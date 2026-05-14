import { LazyStore } from '@tauri-apps/plugin-store';
import type { Pipeline } from './types';

const STORE_FILE = 'pipelines.json';
const KEY_PREFIX = 'pipeline:';
const store = new LazyStore(STORE_FILE);

export const methods = {
	emptyPipeline(id: string, name: string): Pipeline {
		const now = Date.now();
		return { id, name, nodes: [], edges: [], createdAt: now, updatedAt: now };
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
		await store.set(KEY_PREFIX + p.id, p);
		await store.save();
	},

	async remove(id: string): Promise<void> {
		await store.delete(KEY_PREFIX + id);
		await store.save();
	}
};
