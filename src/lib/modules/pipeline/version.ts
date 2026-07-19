import type { Pipeline } from './types';

/** Bumped when a change makes older saved pipelines unrunnable. Old ones are
 * kept as-is rather than migrated -- their routing cannot be remapped without
 * guessing, and a wrong guess reroutes audio silently. */
export const PIPELINE_VERSION = 1;

export function versionOf(pipeline: Pipeline): number {
	return typeof pipeline.version === 'number' ? pipeline.version : 0;
}

/** Saved by an incompatible build: it can be listed and deleted, not opened. */
export function isOutdated(pipeline: Pipeline): boolean {
	return versionOf(pipeline) < PIPELINE_VERSION;
}
