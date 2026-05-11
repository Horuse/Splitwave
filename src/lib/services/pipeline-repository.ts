import { LazyStore } from '@tauri-apps/plugin-store';
import type { Pipeline } from '$lib/domain/pipeline';

const STORE_FILE = 'pipelines.json';
const KEY_PREFIX = 'pipeline:';

// Thin repository over tauri-plugin-store. Keys: `pipeline:<id>` → Pipeline.
// One file, one key per pipeline. Simple and easy to inspect on disk.
export class PipelineRepository {
	private store = new LazyStore(STORE_FILE);

	async list(): Promise<Pipeline[]> {
		const entries = await this.store.entries<Pipeline>();
		return entries
			.filter(([k]) => k.startsWith(KEY_PREFIX))
			.map(([, v]) => v)
			.sort((a, b) => b.updatedAt - a.updatedAt);
	}

	async get(id: string): Promise<Pipeline | null> {
		const v = await this.store.get<Pipeline>(KEY_PREFIX + id);
		return v ?? null;
	}

	async save(p: Pipeline): Promise<void> {
		await this.store.set(KEY_PREFIX + p.id, p);
		await this.store.save();
	}

	async remove(id: string): Promise<void> {
		await this.store.delete(KEY_PREFIX + id);
		await this.store.save();
	}
}
