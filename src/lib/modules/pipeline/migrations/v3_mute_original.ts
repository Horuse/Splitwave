import type { Pipeline } from '../types';

/** Capture used to leave the original audible, and `muteOriginal` now defaults
 * to on for new nodes. Existing capture nodes keep the old behaviour. */
export function migrateMuteOriginal(pipeline: Pipeline): Pipeline {
	return {
		...pipeline,
		nodes: pipeline.nodes.map((n) => {
			if (n.kind !== 'systemAudio' && n.kind !== 'appAudio') return n;
			return { ...n, data: { ...n.data, muteOriginal: false } };
		})
	};
}
