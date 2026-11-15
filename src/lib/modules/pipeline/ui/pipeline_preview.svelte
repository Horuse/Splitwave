<script lang="ts">
	import type { PipelineEdge, PipelineNode } from '@splitwave/nodes';
	import { registry } from '$lib/modules/flow/utils/nodes';
	import { CATEGORY_TEXT } from '$lib/modules/flow/utils/accents';
	import { edgePath } from '$lib/modules/flow/edge_settings.svelte';
	import { Position } from '@xyflow/svelte';

	let { nodes, edges }: { nodes: PipelineNode[]; edges: PipelineEdge[] } = $props();

	const W = 260;
	const H = 120;
	const MIN_SPAN = 900;

	const w = (n: PipelineNode) => n.width ?? n.measured?.width ?? W;
	const h = (n: PipelineNode) => n.height ?? n.measured?.height ?? H;

	let box = $derived.by(() => {
		if (!nodes.length) return null;
		const xs = nodes.flatMap((n) => [n.position.x, n.position.x + w(n)]);
		const ys = nodes.flatMap((n) => [n.position.y, n.position.y + h(n)]);
		const pad = 60;
		const x0 = Math.min(...xs);
		const y0 = Math.min(...ys);
		const bw = Math.max(Math.max(...xs) - x0, MIN_SPAN) + pad * 2;
		const bh = Math.max(Math.max(...ys) - y0, MIN_SPAN / 2) + pad * 2;
		const cx = (x0 + Math.max(...xs)) / 2;
		const cy = (y0 + Math.max(...ys)) / 2;
		return { minX: cx - bw / 2, minY: cy - bh / 2, w: bw, h: bh };
	});

	let byId = $derived(new Map(nodes.map((n) => [n.id, n])));
	let links = $derived(
		edges.flatMap((e) => {
			const a = byId.get(e.source);
			const b = byId.get(e.target);
			if (!a || !b) return [];
			const d = edgePath({
				sourceX: a.position.x + w(a),
				sourceY: a.position.y + h(a) / 2,
				sourcePosition: Position.Right,
				targetX: b.position.x,
				targetY: b.position.y + h(b) / 2,
				targetPosition: Position.Left
			});
			return [{ id: e.id, d }];
		})
	);
</script>

{#if box}
	<svg viewBox="{box.minX} {box.minY} {box.w} {box.h}" preserveAspectRatio="xMidYMid meet" class="h-full w-full" aria-hidden="true">
		{#each links as l (l.id)}
			<path d={l.d} fill="none" stroke="currentColor" stroke-width="1.5" vector-effect="non-scaling-stroke" class="text-neutral-600" />
		{/each}
		{#each nodes as n (n.id)}
			{@const entry = registry[n.kind]}
			<rect
				x={n.position.x}
				y={n.position.y}
				width={w(n)}
				height={h(n)}
				rx="24"
				fill="currentColor"
				fill-opacity="0.15"
				stroke="currentColor"
				stroke-opacity="0.6"
				stroke-width="1"
				vector-effect="non-scaling-stroke"
				class={entry ? CATEGORY_TEXT[entry.category] : 'text-neutral-700'} />
		{/each}
	</svg>
{:else}
	<div class="flex h-full items-center justify-center text-xs text-neutral-800">Empty</div>
{/if}
