import type { AudioDevice } from '$lib/modules/audio/types';
import type { PipelineNode, SpeakerNodeData } from '@splitwave/nodes';
import { registry } from '@splitwave/nodes/catalog';

/** Where a pipeline sends audio: speaker device names, other outputs by node label. */
export function outputLabels(nodes: PipelineNode[], devices: AudioDevice[]): string[] {
	const labels = nodes.flatMap((node) => {
		const entry = registry[node.kind];
		if (entry?.category !== 'output') return [];
		if (node.kind !== 'speaker') return [entry.label];
		const { deviceId } = node.data as SpeakerNodeData;
		if (!deviceId) return [];
		return [devices.find((d) => d.id === deviceId)?.name ?? deviceId];
	});
	return [...new Set(labels)];
}
