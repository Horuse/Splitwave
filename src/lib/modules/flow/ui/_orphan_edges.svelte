<script lang="ts">
	import { ViewportPortal, useStore } from '@xyflow/svelte';
	import { Delete } from '$lib/components/icons';
	import { registry } from '$lib/modules/flow/utils';
	import type { NodeKind } from '$lib/modules/pipeline/types';

	interface Props {
		onDelete: (edgeId: string) => void;
	}
	let { onDelete }: Props = $props();

	// An edge whose handle no longer exists is dropped by xyflow entirely, so the
	// connection stays live in the graph while looking absent on canvas. We draw
	// those ourselves, landing on the node's nearest border and outlining it.
	const store = useStore();

	// handleBounds lives on the non-reactive nodeLookup and is remeasured
	// asynchronously after a node grows or loses handles, so no signal marks the
	// moment an edge becomes orphaned. Poll instead of missing the transition.
	let tick = $state(0);
	$effect(() => {
		const t = setInterval(() => tick++, 200);
		return () => clearInterval(t);
	});

	let active = $state<string | null>(null);
	// The pointer has to cross empty canvas between the line and its button.
	let clearTimer: ReturnType<typeof setTimeout> | undefined;
	function hold(id: string) {
		clearTimeout(clearTimer);
		active = id;
	}
	function release() {
		clearTimeout(clearTimer);
		clearTimer = setTimeout(() => (active = null), 400);
	}

	interface Rect {
		x: number;
		y: number;
		w: number;
		h: number;
	}
	type End = { kind: 'handle'; x: number; y: number } | { kind: 'node'; rect: Rect };

	function resolve(
		nodeId: string,
		handleId: string | null | undefined,
		side: 'source' | 'target'
	): End | null {
		const node = store.nodeLookup.get(nodeId);
		if (!node) return null;
		const w = node.measured.width ?? 0;
		const h = node.measured.height ?? 0;
		if (!w || !h) return null;
		const { x, y } = node.internals.positionAbsolute;

		const bounds = node.internals.handleBounds?.[side] ?? [];
		const hit = handleId ? bounds.find((b) => b.id === handleId) : bounds[0];
		if (hit) {
			return { kind: 'handle', x: x + hit.x + hit.width / 2, y: y + hit.y + hit.height / 2 };
		}
		return { kind: 'node', rect: { x, y, w, h } };
	}

	function centre(end: End): { x: number; y: number } {
		return end.kind === 'handle'
			? { x: end.x, y: end.y }
			: { x: end.rect.x + end.rect.w / 2, y: end.rect.y + end.rect.h / 2 };
	}

	const GAP = 5;

	// Where a ray from the node's centre toward `to` leaves the node box.
	function border(rect: Rect, to: { x: number; y: number }): { x: number; y: number } {
		const cx = rect.x + rect.w / 2;
		const cy = rect.y + rect.h / 2;
		const dx = to.x - cx;
		const dy = to.y - cy;
		if (dx === 0 && dy === 0) return { x: cx, y: cy };
		const scale = Math.min(
			dx === 0 ? Infinity : rect.w / 2 / Math.abs(dx),
			dy === 0 ? Infinity : rect.h / 2 / Math.abs(dy)
		);
		// Clear of the node's own border so the line meets the outline, not the node.
		const len = Math.hypot(dx, dy);
		return { x: cx + dx * scale + (dx / len) * GAP, y: cy + dy * scale + (dy / len) * GAP };
	}

	let orphans = $derived.by(() => {
		tick;
		// Reading the reactive node array keeps the anchors following a drag.
		for (const n of store.nodes) {
			n.position.x;
			n.position.y;
		}

		return store.edges.flatMap((e) => {
			const src = resolve(e.source, e.sourceHandle, 'source');
			const tgt = resolve(e.target, e.targetHandle, 'target');
			if (!src || !tgt) return [];
			if (src.kind === 'handle' && tgt.kind === 'handle') return [];

			const from = src.kind === 'handle' ? src : border(src.rect, centre(tgt));
			const to = tgt.kind === 'handle' ? tgt : border(tgt.rect, centre(src));
			return [
				{
					id: e.id,
					from,
					to,
					mid: { x: (from.x + to.x) / 2, y: (from.y + to.y) / 2 },
					nodes: [
						...(src.kind === 'node' ? [{ id: e.source, rect: src.rect }] : []),
						...(tgt.kind === 'node' ? [{ id: e.target, rect: tgt.rect }] : [])
					]
				}
			];
		});
	});

	let outlined = $derived.by(() => {
		const byNode = new Map<string, { rect: Rect; count: number }>();
		for (const o of orphans) {
			for (const n of o.nodes) {
				const seen = byNode.get(n.id);
				byNode.set(n.id, { rect: n.rect, count: (seen?.count ?? 0) + 1 });
			}
		}
		return [...byNode].map(([id, v]) => {
			const kind = store.nodeLookup.get(id)?.type as NodeKind | undefined;
			const category = kind ? registry[kind]?.category : undefined;
			// A device node has no channels of its own until one is picked; anything
			// else only exposes outputs once something feeds its input.
			return { id, ...v, device: category === 'input' || category === 'output' };
		});
	});

	$effect(() => {
		if (active && !orphans.some((o) => o.id === active)) active = null;
	});
</script>

<ViewportPortal target="back">
	<svg class="absolute overflow-visible z-10" style="width:1px;height:1px;pointer-events:none">
		{#each orphans as o (o.id)}
			<path
				d="M {o.from.x},{o.from.y} L {o.to.x},{o.to.y}"
				fill="none"
				stroke="transparent"
				stroke-width="14"
				style="pointer-events:stroke;cursor:pointer"
				role="button"
				tabindex="-1"
				aria-label="Broken connection"
				onmouseenter={() => hold(o.id)}
				onmouseleave={release}
				onclick={() => hold(o.id)}
				onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && hold(o.id)}
			/>
			<path
				class="orphan"
				class:active={active === o.id}
				d="M {o.from.x},{o.from.y} L {o.to.x},{o.to.y}"
				fill="none"
				stroke="#f59e0b"
				stroke-width="2"
				stroke-dasharray="6 4"
				style="pointer-events:none"
			/>
		{/each}
	</svg>
</ViewportPortal>

<ViewportPortal target="front">
	<svg class="absolute overflow-visible" style="width:1px;height:1px;pointer-events:none">
		{#each outlined as n (n.id)}
			<rect
				class="orphan"
				x={n.rect.x - 2}
				y={n.rect.y - 2}
				width={n.rect.w + 4}
				height={n.rect.h + 4}
				rx="18"
				fill="none"
				stroke="#f59e0b"
				stroke-width="2"
				stroke-dasharray="6 4"
			/>
		{/each}
	</svg>

	{#each outlined as n (n.id)}
		<div
			class="pointer-events-none absolute rounded-lg border border-amber-500/60 bg-amber-500/15 px-2 py-1 text-[10px] leading-snug text-amber-700 dark:text-amber-300"
			style="left:{n.rect.x}px;top:{n.rect.y + n.rect.h + 8}px;width:{n.rect.w}px"
		>
			{n.count === 1 ? 'A cable is' : `${n.count} cables are`} still plugged in here, but there is nowhere
			to take {n.count === 1 ? 'it' : 'them'} right now.
			{#if n.device}
				Pick a device with enough channels, or remove the {n.count === 1 ? 'cable' : 'cables'}.
			{:else}
				This node passes channels through, so its outputs disappear once its input is
				disconnected. Feed it again, or remove the {n.count === 1 ? 'cable' : 'cables'}.
			{/if}
		</div>
	{/each}

	{#each orphans as o (o.id)}
		{#if active === o.id}
			<button
				type="button"
				class="nodrag nopan z-10 pointer-events-auto absolute flex h-5 w-5 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full border border-amber-500 bg-amber-500 text-white shadow-sm hover:bg-amber-600"
				style="left:{o.mid.x}px;top:{o.mid.y}px"
				title="Remove this connection"
				onmouseenter={() => hold(o.id)}
				onmouseleave={release}
				onclick={() => onDelete(o.id)}
			>
				<Delete class="h-3 w-3" />
			</button>
		{/if}
	{/each}
</ViewportPortal>

<style>
	.orphan {
		animation: orphan-blink 1s ease-in-out infinite;
	}

	.orphan.active {
		animation: none;
	}

	@keyframes orphan-blink {
		0%,
		100% {
			opacity: 1;
		}
		50% {
			opacity: 0.25;
		}
	}
</style>
