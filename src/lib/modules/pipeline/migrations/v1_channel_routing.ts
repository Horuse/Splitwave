import type { Pipeline, PipelineEdge } from '../types';
import { withDefaults } from '../defaults';

/** v0 carried one stereo stream per cable on a single unnamed handle per side.
 * v1 gives every channel its own handle, so each old cable becomes two: the
 * pair it always was, now addressable. */
const V0_WIDTH = 2;

/** Named targets (a compressor sidechain) were never part of the stereo pair and
 * keep their handle; only their source end needs a channel. */
function isNamedTarget(edge: PipelineEdge): boolean {
	return !!edge.targetHandle && !/^ch\d+$/.test(edge.targetHandle);
}

function splitEdge(edge: PipelineEdge): PipelineEdge[] {
	if (isNamedTarget(edge)) {
		return [{ ...edge, sourceHandle: 'ch1' }];
	}
	return Array.from({ length: V0_WIDTH }, (_, i) => ({
		...edge,
		id: `${edge.id}-ch${i + 1}`,
		sourceHandle: `ch${i + 1}`,
		targetHandle: `ch${i + 1}`
	}));
}

export function migrateChannelRouting(pipeline: Pipeline): Pipeline {
	return {
		...pipeline,
		nodes: pipeline.nodes.map((n) => ({ ...n, data: withDefaults(n.kind, n.data) })),
		edges: pipeline.edges.flatMap(splitEdge)
	};
}
