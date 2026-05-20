import type { Node as XyNode, Edge as XyEdge } from '@xyflow/svelte';
import type {
	NodeKind,
	PipelineEdge,
	PipelineNode
} from '$lib/modules/pipeline/types';
import { registry } from './nodes';

export function toXyNodes(nodes: PipelineNode[]): XyNode[] {
	return nodes.map((n) => ({
		id: n.id,
		type: n.kind,
		position: n.position,
		data: { ...n.data },
		...(n.width != null && { width: n.width }),
		...(n.height != null && { height: n.height })
	}));
}

export function toXyEdges(edges: PipelineEdge[]): XyEdge[] {
	return edges.map((e) => ({
		id: e.id,
		source: e.source,
		sourceHandle: e.sourceHandle,
		target: e.target,
		targetHandle: e.targetHandle
	}));
}

export function fromXyNodes(xyNodes: XyNode[]): PipelineNode[] {
	return xyNodes.flatMap((n) => {
		const kind = n.type as NodeKind | undefined;
		if (!kind || !(kind in registry)) return [];
		return [
			{
				id: n.id,
				kind,
				position: { x: n.position.x, y: n.position.y },
				data: { ...(n.data as Record<string, unknown>) } as PipelineNode['data'],
				...(n.width != null && { width: n.width }),
				...(n.height != null && { height: n.height })
			}
		];
	});
}

export function fromXyEdges(xyEdges: XyEdge[]): PipelineEdge[] {
	return xyEdges.map((e) => ({
		id: e.id,
		source: e.source,
		sourceHandle: e.sourceHandle ?? undefined,
		target: e.target,
		targetHandle: e.targetHandle ?? undefined
	}));
}
