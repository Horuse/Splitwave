import { LazyStore } from '@tauri-apps/plugin-store';
import type { Pipeline } from './types';
import { PIPELINE_VERSION } from './version';
import { isFromFuture, migrate, migrateSnapshot } from './migrations';
import { pruneDanglingEdges } from './sanitize';
import { appSettings } from '$lib/modules/settings/stores.svelte';

const STORE_FILE = 'pipelines.json';
const KEY_PREFIX = 'pipeline:';
const SNAPSHOT_KEY_PREFIX = 'snapshots:';
const ACTIVE_PIPELINE_KEY = 'activePipelineId';
const store = new LazyStore(STORE_FILE);
let writeQueue: Promise<void> = Promise.resolve();

function enqueueWrite(operation: () => Promise<void>): Promise<void> {
	const result = writeQueue.then(operation, operation);
	writeQueue = result.catch(() => {});
	return result;
}

export interface Snapshot {
	takenAt: number;
	pipeline: Pipeline;
}

export const methods = {
	async list(): Promise<Pipeline[]> {
		const entries = await store.entries<Pipeline>();
		return entries
			.filter(([k]) => k.startsWith(KEY_PREFIX))
			.map(([, pipeline]) => (isFromFuture(pipeline) ? pipeline : pruneDanglingEdges(migrate(pipeline))))
			.sort((a, b) => b.updatedAt - a.updatedAt);
	},

	/** Migrates on read; the result is only persisted once the pipeline is saved,
	 * so opening a v0 pipeline in a build that crashes leaves the original intact. */
	async get(id: string): Promise<Pipeline | null> {
		const stored = await store.get<Pipeline>(KEY_PREFIX + id);
		if (!stored) return null;
		return isFromFuture(stored) ? stored : pruneDanglingEdges(migrate(stored));
	},

	save(p: Pipeline): Promise<void> {
		return enqueueWrite(async () => {
			const clean = pruneDanglingEdges(p);
			const nodes = clean.nodes.map((n) => {
				if (n.kind !== 'fileRecording') return n;
				const data = n.data as Record<string, unknown>;
				return {
					...n,
					data: {
						...data,
						allowOverwrite: data.mode === 'overwrite'
					}
				};
			});
			await store.set(KEY_PREFIX + p.id, { ...clean, nodes, version: PIPELINE_VERSION });
			await store.save();
		});
	},

	remove(id: string): Promise<void> {
		return enqueueWrite(async () => {
			await store.delete(KEY_PREFIX + id);
			await store.delete(SNAPSHOT_KEY_PREFIX + id);
			await store.save();
		});
	},

	async getActivePipelineId(): Promise<string | null> {
		return (await store.get<string>(ACTIVE_PIPELINE_KEY)) ?? null;
	},

	setActivePipelineId(id: string | null): Promise<void> {
		return enqueueWrite(async () => {
			if (id === null) {
				await store.delete(ACTIVE_PIPELINE_KEY);
			} else {
				await store.set(ACTIVE_PIPELINE_KEY, id);
			}
			await store.save();
		});
	},

	async listSnapshots(id: string): Promise<Snapshot[]> {
		const snapshots = (await store.get<Snapshot[]>(SNAPSHOT_KEY_PREFIX + id)) ?? [];
		return snapshots.map((snapshot) => {
			const migrated = migrateSnapshot(snapshot);
			if (isFromFuture(migrated.pipeline)) return migrated;
			return { ...migrated, pipeline: pruneDanglingEdges(migrated.pipeline) };
		});
	},

	addSnapshot(p: Pipeline): Promise<void> {
		return enqueueWrite(async () => {
			const key = SNAPSHOT_KEY_PREFIX + p.id;
			const existing = (await store.get<Snapshot[]>(key)) ?? [];
			existing.push({ takenAt: Date.now(), pipeline: { ...p, version: PIPELINE_VERSION } });
			// Ring-buffer behaviour -- drop oldest.
			const cap = appSettings.maxSnapshots;
			if (existing.length > cap) {
				existing.splice(0, existing.length - cap);
			}
			await store.set(key, existing);
			await store.save();
		});
	}
};
