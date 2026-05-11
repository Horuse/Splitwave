// All known node kinds live here. Adding a new kind = extend this union + add a
// variant to NodeData + register it in flow/node-registry.ts. Existing code does
// not need to be edited.
export type NodeKind = 'input' | 'output';

// Index signature is required so these types are assignable to xyflow's
// `Record<string, unknown>` data slot without a cast.
export interface InputNodeData extends Record<string, unknown> {
	deviceId: string | null;
}

export interface OutputNodeData extends Record<string, unknown> {
	deviceId: string | null;
}

// Discriminated union — each variant is keyed by the parent node's `kind`.
export type NodeDataMap = {
	input: InputNodeData;
	output: OutputNodeData;
};

export type AnyNodeData = NodeDataMap[NodeKind];

export interface PipelineNode<K extends NodeKind = NodeKind> {
	id: string;
	kind: K;
	data: NodeDataMap[K];
	position: { x: number; y: number };
}

export interface PipelineEdge {
	id: string;
	source: string;
	target: string;
}
