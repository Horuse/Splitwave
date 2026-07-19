import type { Pipeline } from './types';

/** An edge whose endpoint node is gone renders as nothing and is unreachable in
 * the editor, so it can only be dropped. */
export function pruneDanglingEdges(pipeline: Pipeline): Pipeline {
	const ids = new Set(pipeline.nodes.map((n) => n.id));
	const edges = pipeline.edges.filter((e) => ids.has(e.source) && ids.has(e.target));
	return edges.length === pipeline.edges.length ? pipeline : { ...pipeline, edges };
}
