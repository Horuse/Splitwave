<script lang="ts">
	import { untrack } from 'svelte';
	import { SvelteFlow, SvelteFlowProvider, type Edge, type Node } from '@xyflow/svelte';
	import ChannelEdge from '$lib/modules/flow/ui/_channel_edge.svelte';
	import ConnectionLine from '$lib/modules/flow/ui/_connection_line.svelte';
	import { canConnect, channelHandle, controlHandle } from '$lib/modules/flow/utils';
	import StubSockets from './stub_sockets.svelte';

	interface Props {
		audio: number;
		control: number;
	}
	let { audio, control }: Props = $props();

	const nodeTypes = { sockets: StubSockets };
	const edgeTypes = { channel: ChannelEdge };

	function build(): { nodes: Node[]; edges: Edge[] } {
		const handles = [
			...Array.from({ length: audio }, (_, i) => channelHandle(i + 1)),
			...Array.from({ length: control }, (_, i) => controlHandle(`c${i + 1}`))
		];
		return {
			nodes: [
				{ id: 'from', type: 'sockets', position: { x: 0, y: 0 }, data: { label: 'From', side: 'source', handles } },
				{ id: 'to', type: 'sockets', position: { x: 320, y: 0 }, data: { label: 'To', side: 'target', handles } }
			],
			edges: handles.map((h) => ({ id: h, type: 'channel', source: 'from', sourceHandle: h, target: 'to', targetHandle: h }))
		};
	}

	let graph = $derived(build());
	let nodes = $state.raw<Node[]>(untrack(() => graph.nodes));
	let edges = $state.raw<Edge[]>(untrack(() => graph.edges));
	$effect.pre(() => {
		nodes = graph.nodes;
		edges = graph.edges;
	});
</script>

<SvelteFlowProvider>
	<div class="h-screen w-full">
		<SvelteFlow
			proOptions={{ hideAttribution: true }}
			class="!bg-background"
			bind:nodes
			bind:edges
			{nodeTypes}
			{edgeTypes}
			connectionLineComponent={ConnectionLine}
			isValidConnection={(c) => canConnect(c.sourceHandle, c.targetHandle)}
			fitView
			fitViewOptions={{ maxZoom: 1.5, padding: 0.3 }} />
	</div>
</SvelteFlowProvider>
