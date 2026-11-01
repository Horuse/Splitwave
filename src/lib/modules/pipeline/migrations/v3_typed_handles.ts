import type { Pipeline } from '../types';

/** Handles gained a signal prefix (`audio:` / `control:`), so a cable can only
 * join sockets of one signal. Every v2 cable carried audio. An unnamed end was
 * the node's whole bus, and the collaborator's `mixed` output was that same bus. */
function typed(handle: string | undefined): string {
	if (!handle || handle === 'mixed') return 'audio:main';
	return `audio:${handle}`;
}

export function migrateTypedHandles(pipeline: Pipeline): Pipeline {
	return {
		...pipeline,
		edges: pipeline.edges.map((e) => ({
			...e,
			sourceHandle: typed(e.sourceHandle),
			targetHandle: typed(e.targetHandle)
		}))
	};
}
