<script lang="ts">
	import { untrack } from 'svelte';
	import { SvelteFlow, SvelteFlowProvider, type Edge, type Node } from '@xyflow/svelte';
	import type { NodeKind } from '$lib/modules/pipeline/types';
	import ChannelEdge from '$lib/modules/flow/ui/_channel_edge.svelte';
	import { canConnect, defaultDataFor, nodeTypes } from '$lib/modules/flow/utils';
	import EnvScope from './env_scope.svelte';
	import { DEFAULT_ENV } from './backend';

	interface Props {
		/** Control source on the left, wired from `out` into `to`. */
		from: NodeKind;
		fromData?: Record<string, unknown>;
		to: NodeKind;
		toHandle: string;
		fromHandle: string;
	}
	let { from, fromData = {}, to, toHandle, fromHandle }: Props = $props();

	const edgeTypes = { channel: ChannelEdge };

	// Built once: the story remounts the canvas when its args change.
	let nodes = $state.raw<Node[]>(
		untrack(() => [
			{ id: 'src', type: from, position: { x: 0, y: 120 }, data: { ...defaultDataFor(from), ...fromData } },
			{ id: 'dst', type: to, position: { x: 360, y: 0 }, data: defaultDataFor(to) }
		])
	);
	let edges = $state.raw<Edge[]>(
		untrack(() => [{ id: 'cable', type: 'channel', source: 'src', sourceHandle: fromHandle, target: 'dst', targetHandle: toHandle }])
	);
</script>

<EnvScope env={DEFAULT_ENV}>
	<SvelteFlowProvider>
		<div class="h-screen w-full">
			<SvelteFlow
				proOptions={{ hideAttribution: true }}
				class="!bg-background"
				bind:nodes
				bind:edges
				{nodeTypes}
				{edgeTypes}
				isValidConnection={(c) => canConnect(c.sourceHandle, c.targetHandle)}
				fitView
				fitViewOptions={{ maxZoom: 1.25, padding: 0.2 }} />
		</div>
	</SvelteFlowProvider>
</EnvScope>
