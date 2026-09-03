import type { Pipeline } from '../types';
import { withDefaults } from '../defaults';

/** `allowOverwrite: boolean` became a three-way `mode` (new/overwrite/append).
 * `true` maps to `overwrite`; `false`/absent stays the `new` default. */
export function migrateFileRecordingMode(pipeline: Pipeline): Pipeline {
	return {
		...pipeline,
		nodes: pipeline.nodes.map((n) => {
			if (n.kind !== 'fileRecording') return n;
			const data = withDefaults(n.kind, n.data) as Record<string, unknown>;
			if (data.allowOverwrite === true) data.mode = 'overwrite';
			delete data.allowOverwrite;
			return { ...n, data };
		})
	};
}
