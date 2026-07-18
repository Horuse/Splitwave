<script lang="ts">
	import { BaseEdge, getBezierPath, type EdgeProps } from '@xyflow/svelte';
	import { channelColor, darken, parseHandle } from '$lib/modules/flow/utils';

	let {
		id,
		sourceX,
		sourceY,
		targetX,
		targetY,
		sourcePosition,
		targetPosition,
		sourceHandleId,
		markerEnd
	}: EdgeProps = $props();

	// Vertical gap between the two conductors of a stereo cable. The bezier runs
	// horizontally at both ends, so translating copies reads as a parallel pair.
	const CONDUCTOR_GAP = 3;
	const PIN_W = 3;
	const PIN_H = 6;

	let wire = $derived(sourceHandleId ? parseHandle(sourceHandleId) : null);
	let colors = $derived(
		wire === null
			? []
			: Array.from({ length: wire.width }, (_, i) => channelColor(wire.ch - 1 + i))
	);
	// Conductor offsets are symmetric around the path: [0] for mono, [-g, +g].
	let offsets = $derived(
		colors.map((_, i) => (colors.length === 1 ? 0 : (i - (colors.length - 1) / 2) * CONDUCTOR_GAP))
	);

	let path = $derived(
		getBezierPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition })[0]
	);
</script>

{#if colors.length === 0}
	<BaseEdge {id} {path} {markerEnd} />
{:else}
	<!-- BaseEdge stays underneath for xyflow's hit area and selection styling. -->
	<BaseEdge {id} {path} style="stroke:transparent;stroke-width:6px" {markerEnd} />
	<g class="pointer-events-none">
		{#each colors as color, i (i)}
			<path d={path} fill="none" stroke={color} stroke-width="2" transform="translate(0,{offsets[i]})" />
		{/each}
		{#each colors as color, i (i)}
			{#each [[sourceX, sourceY], [targetX, targetY]] as [x, y], end (end)}
				<rect
					x={x - PIN_W / 2}
					y={y + offsets[i] - PIN_H / 2}
					width={PIN_W}
					height={PIN_H}
					rx="1"
					fill={color}
					stroke={darken(color)}
					stroke-width="0.5"
				/>
			{/each}
		{/each}
	</g>
{/if}
