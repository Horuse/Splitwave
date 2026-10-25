<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import { SvelteFlow, SvelteFlowProvider, type Edge, type Node } from '@xyflow/svelte';
	import type { NodeKind } from '$lib/modules/pipeline/types';
	import { nodeTypes, registry } from '$lib/modules/flow/utils';
	import ChannelEdge from '$lib/modules/flow/ui/_channel_edge.svelte';
	import StubSource from './stub_source.svelte';
	import EnvScope from './env_scope.svelte';
	import { setNodeDataLookup, STORY_NODE_ID } from './backend';
	import { startSignal } from './signal';
	import { splitArgs } from './args';

	interface Props {
		kind: NodeKind;
		args: Record<string, unknown>;
	}
	let { kind, args }: Props = $props();

	const NODE_ID = STORY_NODE_ID;
	const STUB_ID = 'story-source';
	const types = { ...nodeTypes, stub: StubSource };
	const edgeTypes = { channel: ChannelEdge };

	let split = $derived(splitArgs(args));
	// Engine answers are read once on mount, so a changed engine remounts the node.
	let envKey = $derived(JSON.stringify(split.env));

	function build(): { nodes: Node[]; edges: Edge[] } {
		const { data, wiredInputs } = split;
		const entry = registry[kind];
		const node: Node = {
			id: NODE_ID,
			type: kind,
			position: { x: wiredInputs > 0 ? 220 : 0, y: 0 },
			data: { ...structuredClone(entry.defaultData), ...data },
			...(entry.defaultSize ?? {})
		};
		if (wiredInputs <= 0 || entry.category === 'input') return { nodes: [node], edges: [] };
		const stub: Node = { id: STUB_ID, type: 'stub', position: { x: 0, y: 0 }, data: { channels: wiredInputs } };
		const edges: Edge[] = Array.from({ length: wiredInputs }, (_, i) => ({
			id: `wire-${i + 1}`,
			type: 'channel',
			source: STUB_ID,
			sourceHandle: `ch${i + 1}`,
			target: NODE_ID,
			targetHandle: `ch${i + 1}`
		}));
		return { nodes: [stub, node], edges };
	}

	let graph = $derived(build());
	let nodes = $state.raw<Node[]>(untrack(() => graph.nodes));
	let edges = $state.raw<Edge[]>(untrack(() => graph.edges));
	$effect.pre(() => {
		nodes = graph.nodes;
		edges = graph.edges;
	});

	setNodeDataLookup((id) => nodes.find((n) => n.id === id)?.data as Record<string, unknown> | undefined);
	onDestroy(() => setNodeDataLookup(() => undefined));

	$effect(() => startSignal({ nodeId: NODE_ID, wideScope: kind === 'spectrum', running: split.env.running }, split.signal));
</script>

{#key envKey}
	<EnvScope env={split.env}>
		<SvelteFlowProvider>
			<div class="h-screen w-full">
				<SvelteFlow
					proOptions={{ hideAttribution: true }}
					class="!bg-background"
					bind:nodes
					bind:edges
					nodeTypes={types}
					{edgeTypes}
					fitView
					fitViewOptions={{ maxZoom: 1.25, padding: 0.2 }} />
			</div>
		</SvelteFlowProvider>
	</EnvScope>
{/key}
