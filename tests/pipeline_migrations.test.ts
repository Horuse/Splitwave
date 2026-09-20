import { describe, expect, it } from 'bun:test';
import { withDefaults } from '../src/lib/modules/pipeline/defaults';
import { isFromFuture, migrate, migrateSnapshot, MIGRATIONS } from '../src/lib/modules/pipeline/migrations';
import { migrateChannelRouting } from '../src/lib/modules/pipeline/migrations/v1_channel_routing';
import { migrateFileRecordingMode } from '../src/lib/modules/pipeline/migrations/v2_file_recording_mode';
import type { Pipeline } from '../src/lib/modules/pipeline/types';
import { PIPELINE_VERSION } from '../src/lib/modules/pipeline/version';

function pipeline(overrides: Partial<Pipeline> = {}): Pipeline {
	return {
		id: 'pipeline',
		name: 'Migration fixture',
		createdAt: 10,
		updatedAt: 20,
		nodes: [],
		edges: [],
		...overrides
	};
}

describe('pipeline migration registry', () => {
	it('has one ordered step for every schema version', () => {
		expect(MIGRATIONS.map((migration) => migration.to)).toEqual(Array.from({ length: PIPELINE_VERSION }, (_, index) => index + 1));
	});

	it('leaves current and future pipelines untouched', () => {
		const current = pipeline({ version: PIPELINE_VERSION });
		const future = pipeline({ version: PIPELINE_VERSION + 1 });
		expect(migrate(current)).toBe(current);
		expect(migrate(future)).toBe(future);
		expect(isFromFuture(current)).toBe(false);
		expect(isFromFuture(future)).toBe(true);
	});
});

describe('v0 channel-routing migration', () => {
	it('splits an unnamed stereo cable into addressable channel edges', () => {
		const source = pipeline({
			nodes: [
				{ id: 'mic', kind: 'microphone', data: {}, position: { x: 1, y: 2 } },
				{ id: 'speaker', kind: 'speaker', data: {}, position: { x: 3, y: 4 } }
			] as Pipeline['nodes'],
			edges: [{ id: 'cable', source: 'mic', target: 'speaker' }]
		});

		const migrated = migrateChannelRouting(source);
		expect(migrated.edges).toEqual([
			{ id: 'cable-ch1', source: 'mic', target: 'speaker', sourceHandle: 'ch1', targetHandle: 'ch1' },
			{ id: 'cable-ch2', source: 'mic', target: 'speaker', sourceHandle: 'ch2', targetHandle: 'ch2' }
		]);
		expect(migrated.nodes[0]).toEqual({
			id: 'mic',
			kind: 'microphone',
			data: withDefaults('microphone', {}),
			position: { x: 1, y: 2 }
		});
		expect(source.edges).toEqual([{ id: 'cable', source: 'mic', target: 'speaker' }]);
	});

	it('preserves a named sidechain target and assigns only its source channel', () => {
		const source = pipeline({
			edges: [
				{
					id: 'key',
					source: 'key-source',
					target: 'compressor',
					targetHandle: 'sidechain'
				}
			]
		});
		expect(migrateChannelRouting(source).edges).toEqual([
			{
				id: 'key',
				source: 'key-source',
				target: 'compressor',
				sourceHandle: 'ch1',
				targetHandle: 'sidechain'
			}
		]);
	});
});

describe('v1 file-recording migration', () => {
	it('maps the legacy overwrite flag and fills current defaults', () => {
		const source = pipeline({
			version: 1,
			nodes: [
				{
					id: 'overwrite',
					kind: 'fileRecording',
					data: { allowOverwrite: true, filePath: '/tmp/out.wav' },
					position: { x: 0, y: 0 }
				},
				{
					id: 'new',
					kind: 'fileRecording',
					data: { allowOverwrite: false },
					position: { x: 1, y: 0 }
				}
			] as Pipeline['nodes']
		});

		const migrated = migrateFileRecordingMode(source);
		expect(migrated.nodes[0].data).toMatchObject({
			filePath: '/tmp/out.wav',
			mode: 'overwrite',
			allowOverwrite: true,
			channels: 2,
			sampleRate: 48_000
		});
		expect(migrated.nodes[1].data).toMatchObject({ mode: 'new', allowOverwrite: false });
		expect(source.nodes[0].data).toEqual({ allowOverwrite: true, filePath: '/tmp/out.wav' });
	});

	it('fills fields missing inside every recording-format variant', () => {
		const cases = [
			[{ kind: 'wav' }, { kind: 'wav', bitDepth: 'f32' }],
			[{ kind: 'flac' }, { kind: 'flac', bitDepth: 'i24', compression: 'default' }],
			[{ kind: 'opus' }, { kind: 'opus', bitrate: 128_000, application: 'audio' }],
			[{ kind: 'mp3' }, { kind: 'mp3', bitrateKbps: 192 }],
			[{ kind: 'aac' }, { kind: 'aac', bitrate: 192_000 }],
			[{ kind: 'aiff' }, { kind: 'aiff', bitDepth: 'i24' }]
		] as const;

		for (const [format, expected] of cases) {
			const data = withDefaults('fileRecording', { format });
			expect(data.format).toEqual(expected);
		}
	});
});

describe('complete pipeline migration', () => {
	it('applies v0 through v2 exactly once and preserves pipeline metadata', () => {
		const source = pipeline({
			nodes: [
				{
					id: 'record',
					kind: 'fileRecording',
					data: { allowOverwrite: true },
					position: { x: 4, y: 8 },
					width: 320
				}
			] as Pipeline['nodes'],
			edges: [{ id: 'recording', source: 'input', target: 'record' }]
		});

		const migrated = migrate(source);
		expect(migrated.version).toBe(PIPELINE_VERSION);
		expect(migrated.id).toBe(source.id);
		expect(migrated.createdAt).toBe(10);
		expect(migrated.updatedAt).toBe(20);
		expect(migrated.nodes[0].width).toBe(320);
		expect(migrated.nodes[0].data).toMatchObject({ mode: 'overwrite', allowOverwrite: true });
		expect(migrated.edges.map((edge) => edge.id)).toEqual(['recording-ch1', 'recording-ch2']);
		expect(migrate(migrated)).toBe(migrated);
	});

	it('starts at the declared version instead of replaying older steps', () => {
		const v1 = pipeline({
			version: 1,
			nodes: [
				{
					id: 'record',
					kind: 'fileRecording',
					data: { allowOverwrite: false },
					position: { x: 0, y: 0 }
				}
			] as Pipeline['nodes'],
			edges: [
				{
					id: 'already-routed',
					source: 'input',
					target: 'record',
					sourceHandle: 'ch1',
					targetHandle: 'ch1'
				}
			]
		});

		const migrated = migrate(v1);
		expect(migrated.edges).toEqual(v1.edges);
		expect(migrated.nodes[0].data).toMatchObject({ mode: 'new', allowOverwrite: false });
	});

	it('migrates pipelines stored inside snapshots and preserves future snapshots', () => {
		const legacy = {
			takenAt: 123,
			pipeline: pipeline({ edges: [{ id: 'old', source: 'a', target: 'b' }] })
		};
		const migrated = migrateSnapshot(legacy);
		expect(migrated.takenAt).toBe(123);
		expect(migrated.pipeline.version).toBe(PIPELINE_VERSION);
		expect(migrated.pipeline.edges.map((edge) => edge.id)).toEqual(['old-ch1', 'old-ch2']);
		expect(legacy.pipeline.version).toBeUndefined();

		const future = {
			takenAt: 456,
			pipeline: pipeline({ version: PIPELINE_VERSION + 1 })
		};
		expect(migrateSnapshot(future)).toBe(future);
	});
});
