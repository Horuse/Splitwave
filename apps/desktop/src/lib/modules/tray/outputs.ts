import type { PipelineNode } from '@splitwave/nodes';
import { registry } from '@splitwave/nodes/catalog';
import { wantedId, type NameMemory } from './names';

/** Where a pipeline sends audio: speaker device names, other outputs by node label. */
export function outputLabels(nodes: PipelineNode[], names: NameMemory): string[] {
	const labels = nodes.flatMap((node) => {
		const entry = registry[node.kind];
		if (entry?.category !== 'output') return [];
		if (node.kind !== 'speaker') return [entry.label];
		const id = wantedId(node);
		return id ? [names.of(id)] : [];
	});
	return [...new Set(labels)];
}

/** What the engine is waiting for, named as the user knows it. */
export function waitingLabels(nodes: PipelineNode[], names: NameMemory): string[] {
	const labels = nodes.map((node) => {
		const id = wantedId(node);
		return id ? names.of(id) : (registry[node.kind]?.label ?? node.kind);
	});
	return [...new Set(labels)];
}
