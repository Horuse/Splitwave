import { beforeEach, describe, expect, it, mock } from 'bun:test';
import type { Pipeline } from '../src/lib/modules/pipeline/types';
import { PIPELINE_VERSION } from '../src/lib/modules/pipeline/version';

const persisted = new Map<string, unknown>();
const appSettings = { maxSnapshots: 2, includePreReleases: false };

mock.module('@tauri-apps/plugin-store', () => ({
	LazyStore: class {
		async entries<T>(): Promise<[string, T][]> {
			return [...persisted.entries()] as [string, T][];
		}
		async get<T>(key: string): Promise<T | undefined> {
			return persisted.get(key) as T | undefined;
		}
		async set(key: string, value: unknown): Promise<void> {
			persisted.set(key, value);
		}
		async delete(key: string): Promise<void> {
			persisted.delete(key);
		}
		async save(): Promise<void> {}
	}
}));
mock.module('$lib/modules/settings/stores.svelte', () => ({ appSettings }));

const { methods } = await import('../src/lib/modules/pipeline/methods');

function pipeline(id: string, overrides: Partial<Pipeline> = {}): Pipeline {
	return {
		id,
		name: id,
		createdAt: 1,
		updatedAt: 1,
		nodes: [],
		edges: [],
		...overrides
	};
}

beforeEach(() => {
	persisted.clear();
	appSettings.maxSnapshots = 2;
});

describe('pipeline persistence migrations', () => {
	it('list migrates supported entries, preserves future entries, and ignores other keys', async () => {
		persisted.set('pipeline:legacy', pipeline('legacy', { updatedAt: 10 }));
		const future = pipeline('future', { version: PIPELINE_VERSION + 1, updatedAt: 20 });
		persisted.set('pipeline:future', future);
		persisted.set('activePipelineId', 'legacy');

		const listed = await methods.list();
		expect(listed.map((entry) => entry.id)).toEqual(['future', 'legacy']);
		expect(listed[0]).toBe(future);
		expect(listed[1].version).toBe(PIPELINE_VERSION);
	});

	it('get applies migrations and removes dangling edges without overwriting storage', async () => {
		const legacy = pipeline('legacy', {
			nodes: [{ id: 'source', kind: 'microphone', data: {}, position: { x: 0, y: 0 } }] as Pipeline['nodes'],
			edges: [
				{ id: 'dangling', source: 'source', target: 'missing' },
				{ id: 'self', source: 'source', target: 'source' }
			]
		});
		persisted.set('pipeline:legacy', legacy);

		const loaded = await methods.get('legacy');
		expect(loaded?.version).toBe(PIPELINE_VERSION);
		expect(loaded?.edges.map((edge) => edge.id)).toEqual(['self-ch1', 'self-ch2']);
		expect(persisted.get('pipeline:legacy')).toBe(legacy);
	});

	it('migrates legacy snapshots and stamps new snapshots with the current version', async () => {
		persisted.set('snapshots:p', [{ takenAt: 10, pipeline: pipeline('p', { edges: [{ id: 'old', source: 'a', target: 'b' }] }) }]);
		const loaded = await methods.listSnapshots('p');
		expect(loaded[0].pipeline.version).toBe(PIPELINE_VERSION);
		// Both endpoints are missing, so migration runs first and sanitization removes the split edges.
		expect(loaded[0].pipeline.edges).toEqual([]);

		await methods.addSnapshot(pipeline('p'));
		const stored = persisted.get('snapshots:p') as Array<{ takenAt: number; pipeline: Pipeline }>;
		expect(stored.at(-1)?.pipeline.version).toBe(PIPELINE_VERSION);
	});

	it('caps snapshot history by dropping the oldest records', async () => {
		persisted.set('snapshots:p', [
			{ takenAt: 1, pipeline: pipeline('p', { version: PIPELINE_VERSION }) },
			{ takenAt: 2, pipeline: pipeline('p', { version: PIPELINE_VERSION }) }
		]);
		await methods.addSnapshot(pipeline('p'));

		const stored = persisted.get('snapshots:p') as Array<{ takenAt: number; pipeline: Pipeline }>;
		expect(stored).toHaveLength(2);
		expect(stored.map((snapshot) => snapshot.takenAt)).not.toContain(1);
	});

	it('serializes concurrent snapshot writes without losing either edit', async () => {
		appSettings.maxSnapshots = 10;
		await Promise.all([methods.addSnapshot(pipeline('p', { name: 'first' })), methods.addSnapshot(pipeline('p', { name: 'second' }))]);

		const stored = persisted.get('snapshots:p') as Array<{ pipeline: Pipeline }>;
		expect(stored.map((snapshot) => snapshot.pipeline.name)).toEqual(['first', 'second']);
	});

	it('save writes the current version and the legacy overwrite compatibility flag', async () => {
		await methods.save(
			pipeline('recording', {
				nodes: [
					{
						id: 'file',
						kind: 'fileRecording',
						data: { mode: 'overwrite' },
						position: { x: 0, y: 0 }
					}
				] as Pipeline['nodes']
			})
		);
		const stored = persisted.get('pipeline:recording') as Pipeline;
		expect(stored.version).toBe(PIPELINE_VERSION);
		expect(stored.nodes[0].data).toMatchObject({ mode: 'overwrite', allowOverwrite: true });
	});
});
