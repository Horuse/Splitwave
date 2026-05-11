<script lang="ts">
	import {
		SvelteFlow,
		Background,
		Controls,
		useSvelteFlow,
		type Node as XyNode,
		type Edge as XyEdge,
		type Connection
	} from '@xyflow/svelte';
	import '@xyflow/svelte/dist/style.css';
	import { createId } from '@paralleldrive/cuid2';
	import { onDestroy, untrack } from 'svelte';
	import type { Pipeline } from '$lib/domain/pipeline';
	import type { NodeKind } from '$lib/domain/audio-node';
	import { DND_MIME, nodeRegistry, xyNodeTypes } from '$lib/flow/node-registry';
	import { toXyNodes, toXyEdges, fromXyNodes, fromXyEdges } from '$lib/flow/adapter';
	import { useAppStore } from '$lib/stores/app-store.svelte';

	let { pipeline }: { pipeline: Pipeline } = $props();

	const store = useAppStore();

	// Seed once from prop; afterwards the editor owns its own state.
	// xyflow recommends $state.raw — it replaces array refs internally.
	let nodes = $state.raw<XyNode[]>(untrack(() => toXyNodes(pipeline.nodes)));
	let edges = $state.raw<XyEdge[]>(untrack(() => toXyEdges(pipeline.edges)));

	const flow = useSvelteFlow();

	function onConnect(connection: Connection) {
		if (!connection.source || !connection.target) return;
		edges = [
			...edges,
			{ id: createId(), source: connection.source, target: connection.target }
		];
	}

	function onDragOver(event: DragEvent) {
		event.preventDefault();
		if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
	}

	function onDrop(event: DragEvent) {
		event.preventDefault();
		const kind = event.dataTransfer?.getData(DND_MIME) as NodeKind | undefined;
		if (!kind || !(kind in nodeRegistry)) return;
		const position = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
		addNode(kind, position);
	}

	function addNode(kind: NodeKind, position?: { x: number; y: number }) {
		// Default position: spread new nodes so they don't stack at (0,0).
		const fallback = { x: 100 + nodes.length * 40, y: 100 + nodes.length * 40 };
		nodes = [
			...nodes,
			{
				id: createId(),
				type: kind,
				position: position ?? fallback,
				data: { ...nodeRegistry[kind].defaultData }
			}
		];
	}

	function getSnapshot(): Pipeline {
		return {
			id: pipeline.id,
			name: pipeline.name,
			createdAt: pipeline.createdAt,
			nodes: fromXyNodes(nodes),
			edges: fromXyEdges(edges),
			updatedAt: Date.now()
		};
	}

	// Register imperative actions on mount; nothing reactive happens here.
	store.editorActions = { addNode, getSnapshot };

	// Debounced auto-save. Reading nodes/edges subscribes to them; the body
	// only manipulates a setTimeout, no other $state writes — so this cannot
	// cascade back into the xyflow reactive graph.
	let saveTimer: ReturnType<typeof setTimeout> | undefined;
	$effect(() => {
		nodes;
		edges;
		clearTimeout(saveTimer);
		saveTimer = setTimeout(() => {
			untrack(() => store.savePipeline(getSnapshot()));
		}, 500);
		return () => clearTimeout(saveTimer);
	});

	onDestroy(() => {
		clearTimeout(saveTimer);
		if (store.editorActions?.getSnapshot === getSnapshot) {
			store.editorActions = null;
		}
	});
</script>

<div
	class="flex-1"
	role="region"
	aria-label="Flow editor"
	ondragover={onDragOver}
	ondrop={onDrop}
>
	<SvelteFlow class="!bg-background" bind:nodes bind:edges nodeTypes={xyNodeTypes} onconnect={onConnect} fitView>
		<Background />
		<Controls />
	</SvelteFlow>
</div>
