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
		selected,
		markerEnd
	}: EdgeProps = $props();

	const PIN_W = 3;
	const PIN_H = 6;

	let channel = $derived(sourceHandleId ? parseHandle(sourceHandleId) : null);
	let color = $derived(channel === null ? null : channelColor(channel - 1));

	let path = $derived(
		getBezierPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition })[0]
	);
</script>

{#if color === null}
	<BaseEdge {id} {path} {markerEnd} />
{:else}
	<!-- BaseEdge stays underneath for xyflow's hit area and selection styling. -->
	<BaseEdge {id} {path} style="stroke:transparent;stroke-width:6px" {markerEnd} />
	<g class="pointer-events-none" opacity={selected ? 0.4 : 1}>
		<path d={path} fill="none" stroke={color} stroke-width="2" />
		{#each [[sourceX, sourceY], [targetX, targetY]] as [x, y], end (end)}
			<rect
				x={x - PIN_W / 2}
				y={y - PIN_H / 2}
				width={PIN_W}
				height={PIN_H}
				rx="1"
				fill={color}
				stroke={darken(color)}
				stroke-width="0.5"
			/>
		{/each}
	</g>
{/if}
