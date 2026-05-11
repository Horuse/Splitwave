import type { PipelineEdge, PipelineNode } from './audio-node';

export interface Pipeline {
	id: string;
	name: string;
	nodes: PipelineNode[];
	edges: PipelineEdge[];
	createdAt: number;
	updatedAt: number;
}

export function emptyPipeline(id: string, name: string): Pipeline {
	const now = Date.now();
	return {
		id,
		name,
		nodes: [],
		edges: [],
		createdAt: now,
		updatedAt: now
	};
}
