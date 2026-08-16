import type { Pipeline } from './types';

/** Bumped when stored pipelines need reshaping; each bump gets a step in
 * `./migrations`. Additive field changes do not need a bump -- `withDefaults`
 * fills them in on read. */
export const PIPELINE_VERSION = 2;

export function versionOf(pipeline: Pipeline): number {
	return typeof pipeline.version === 'number' ? pipeline.version : 0;
}
