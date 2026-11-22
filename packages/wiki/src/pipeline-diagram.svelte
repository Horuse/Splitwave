<script lang="ts" module>
	import type { AnyNodeData, NodeKind } from '@splitwave/nodes';

	export interface DiagramNode {
		id: string;
		kind: NodeKind;
		x: number;
		y: number;
		/** Overrides on top of the node's defaults. */
		data?: Partial<AnyNodeData> & Record<string, unknown>;
	}

	/** An audio cable carries `channels` (stereo by default); `sidechain` lands on the effect's key input.
	 * A control cable names its sockets, such as `control:out` to `control:open`. */
	export type DiagramEdge =
		{ from: string; to: string; channels?: number; sidechain?: boolean } | { from: string; to: string; control: { from: string; to: string } };
</script>

<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { SvelteFlow, SvelteFlowProvider, type Edge, type Node } from '@xyflow/svelte';
	import '@xyflow/svelte/dist/style.css';
	import { nodeTypes } from '@splitwave/nodes/node-types';
	import { defaultDataFor } from '@splitwave/nodes/catalog';
	import { createDemoPort, DEFAULT_SIGNAL, setEnginePort, startSignal } from '@splitwave/nodes/engine';
	import { channelHandle, SIDECHAIN_HANDLE } from '@splitwave/nodes/ui';
	import DiagramEdgeView from './diagram-edge.svelte';
	import DiagramFit from './diagram-fit.svelte';

	interface Props {
		nodes: DiagramNode[];
		edges: DiagramEdge[];
		/** Reserved before the canvas mounts, so the article does not jump. */
		height?: number;
		/** Short description for screen readers and search snippets. */
		caption?: string;
	}

	let { nodes: spec, edges: wires, height = 320, caption }: Props = $props();

	// Real nodes on a demo engine: meters move on a synthetic signal, nothing is played.
	const port = createDemoPort();
	setEnginePort(port);

	function build(): { nodes: Node[]; edges: Edge[] } {
		const nodes: Node[] = spec.map((n) => ({
			id: n.id,
			type: n.kind,
			position: { x: n.x, y: n.y },
			data: { ...defaultDataFor(n.kind), ...(n.data ?? {}) },
			draggable: false,
			selectable: false,
			connectable: false
		}));
		const edges: Edge[] = wires.flatMap((w, i) => {
			if ('control' in w) {
				return [{ id: `c${i}`, type: 'diagram', source: w.from, sourceHandle: w.control.from, target: w.to, targetHandle: w.control.to }];
			}
			const count = w.channels ?? 2;
			return Array.from({ length: count }, (_, c) => ({
				id: `a${i}-${c}`,
				type: 'diagram',
				source: w.from,
				sourceHandle: channelHandle(c + 1),
				target: w.to,
				targetHandle: w.sidechain ? SIDECHAIN_HANDLE : channelHandle(c + 1)
			}));
		});
		return { nodes, edges };
	}

	let graph = untrack(build);
	let nodes = $state.raw<Node[]>(graph.nodes);
	let edges = $state.raw<Edge[]>(graph.edges);

	let mounted = $state(false);
	let dark = $state(false);

	onMount(() => {
		mounted = true;
		// Its own .dark scope re-derives the app's scale, whatever background the page uses.
		const root = document.documentElement;
		const sync = () => (dark = root.classList.contains('dark'));
		sync();
		const observer = new MutationObserver(sync);
		observer.observe(root, { attributes: true, attributeFilter: ['class'] });
		const stops = spec.map((n) => startSignal({ nodeId: n.id, wideScope: n.kind === 'spectrum', running: true }, DEFAULT_SIGNAL, port.emit));
		return () => {
			observer.disconnect();
			stops.forEach((stop) => stop());
		};
	});
</script>

<figure class="not-prose my-6">
	<div
		class={['relative overflow-hidden rounded-2xl border border-neutral-300 bg-background text-base leading-normal', dark && 'dark']}
		style:height="{height}px"
		role="img"
		aria-label={caption ?? 'Splitwave pipeline diagram'}>
		{#if mounted}
			<SvelteFlowProvider>
				<SvelteFlow
					bind:nodes
					bind:edges
					{nodeTypes}
					edgeTypes={{ diagram: DiagramEdgeView }}
					proOptions={{ hideAttribution: true }}
					nodesDraggable={false}
					nodesConnectable={false}
					elementsSelectable={false}
					zoomOnScroll={false}
					zoomOnPinch={false}
					zoomOnDoubleClick={false}
					preventScrolling={false}
					class="!bg-transparent">
					<DiagramFit />
				</SvelteFlow>
			</SvelteFlowProvider>
		{/if}
	</div>
	{#if caption}
		<figcaption class="mt-2 text-center text-sm text-neutral-900">{caption}</figcaption>
	{/if}
</figure>
