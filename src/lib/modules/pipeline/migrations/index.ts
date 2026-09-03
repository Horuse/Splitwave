import type { Pipeline } from '../types';
import { PIPELINE_VERSION, versionOf } from '../version';
import { migrateChannelRouting } from './v1_channel_routing';
import { migrateFileRecordingMode } from './v2_file_recording_mode';

export interface Migration {
	/** Version this step produces; steps run in ascending order. */
	to: number;
	migrate: (pipeline: Pipeline) => Pipeline;
}

export const MIGRATIONS: Migration[] = [
	{ to: 1, migrate: migrateChannelRouting },
	{ to: 2, migrate: migrateFileRecordingMode }
];

/** Runs every step above the pipeline's own version. Additive field changes
 * need no step at all -- `withDefaults` covers those on read. */
export function migrate(pipeline: Pipeline): Pipeline {
	const from = versionOf(pipeline);
	if (from >= PIPELINE_VERSION) return pipeline;
	const migrated = MIGRATIONS.filter((m) => m.to > from).reduce((p, m) => m.migrate(p), pipeline);
	return { ...migrated, version: PIPELINE_VERSION };
}

/** Saved by a newer build: its shape is unknown here, so it stays unopened. */
export function isFromFuture(pipeline: Pipeline): boolean {
	return versionOf(pipeline) > PIPELINE_VERSION;
}
