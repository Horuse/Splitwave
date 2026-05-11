import type { Component } from 'svelte';
import type { NodeTypes } from '@xyflow/svelte';
import type { NodeKind, AnyNodeData } from '$lib/domain/audio-node';
import InputNode from './nodes/InputNode.svelte';
import OutputNode from './nodes/OutputNode.svelte';

// MIME type used during drag-and-drop from the sidebar.
export const DND_MIME = 'application/x-betteraudio-nodekind';

export interface NodeRegistryEntry {
	label: string;
	component: Component<any>;
	defaultData: AnyNodeData;
}

// Single source of truth for node kinds. Adding a new kind = add an entry here.
export const nodeRegistry: Record<NodeKind, NodeRegistryEntry> = {
	input: {
		label: 'Input',
		component: InputNode,
		defaultData: { deviceId: null }
	},
	output: {
		label: 'Output',
		component: OutputNode,
		defaultData: { deviceId: null }
	}
};

// Map sent to <SvelteFlow nodeTypes={...} />.
export const xyNodeTypes: NodeTypes = Object.fromEntries(
	Object.entries(nodeRegistry).map(([kind, entry]) => [kind, entry.component])
);

export const nodeKinds: NodeKind[] = Object.keys(nodeRegistry) as NodeKind[];
