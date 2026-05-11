import type { Node as XyNode, Edge as XyEdge } from '@xyflow/svelte';
import type { NodeKind } from '$lib/domain/audio-node';
import type { PipelineEdge, PipelineNode } from '$lib/domain/audio-node';

// Domain → xyflow.
export function toXyNodes(nodes: PipelineNode[]): XyNode[] {
	return nodes.map((n) => ({
		id: n.id,
		type: n.kind,
		position: n.position,
		data: { ...n.data }
	}));
}

export function toXyEdges(edges: PipelineEdge[]): XyEdge[] {
	return edges.map((e) => ({ id: e.id, source: e.source, target: e.target }));
}

// xyflow → domain (strip UI-only fields like measured/selected/dragging).
export function fromXyNodes(xyNodes: XyNode[]): PipelineNode[] {
	return xyNodes.map((n) => ({
		id: n.id,
		kind: (n.type ?? 'input') as NodeKind,
		position: { x: n.position.x, y: n.position.y },
		data: { ...(n.data as Record<string, unknown>) } as PipelineNode['data']
	}));
}

export function fromXyEdges(xyEdges: XyEdge[]): PipelineEdge[] {
	return xyEdges.map((e) => ({ id: e.id, source: e.source, target: e.target }));
}
