import { createId } from '@paralleldrive/cuid2';
import type { Pipeline, PipelineEdge, PipelineNode } from '$lib/modules/pipeline/types';
import { withDefaults } from '$lib/modules/pipeline/defaults';
import { PIPELINE_VERSION } from '$lib/modules/pipeline/version';
import type { Template } from './types';

export function instantiate(template: Template, name: string): Pipeline {
	// Fresh ids per instantiation: two pipelines from one template must not
	// share node ids, since the engine keys effect state by them.
	const ids = new Map(template.nodes.map((n) => [n.key, createId()]));
	const now = Date.now();

	const nodes: PipelineNode[] = template.nodes.map((n) => ({
		id: ids.get(n.key)!,
		kind: n.kind,
		position: n.position,
		data: withDefaults(n.kind, n.data ?? {})
	}));

	const edges: PipelineEdge[] = template.edges.map((e) => {
		const source = ids.get(e.from)!;
		const target = ids.get(e.to)!;
		const sourceHandle = `ch${e.fromCh ?? 1}`;
		const targetHandle = e.toHandle ?? `ch${e.toCh ?? 1}`;
		return {
			id: `${source}-${sourceHandle}-${target}-${targetHandle}`,
			source,
			sourceHandle,
			target,
			targetHandle
		};
	});

	return {
		id: createId(),
		name,
		nodes,
		edges,
		createdAt: now,
		updatedAt: now,
		version: PIPELINE_VERSION,
		sourceTemplateId: template.id,
		sourceTemplateVersion: template.version
	};
}
