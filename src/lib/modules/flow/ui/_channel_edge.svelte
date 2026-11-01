<script lang="ts">
	import { BaseEdge, getBezierPath, getSmoothStepPath, getStraightPath, type EdgeProps } from '@xyflow/svelte';
	import { CONTROL_DASH, darken, handleColor, signalOf } from '$lib/modules/flow/utils';
	import { edgeSettings } from '../edge_settings.svelte';

	let { id, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, sourceHandleId, selected, markerEnd }: EdgeProps = $props();

	const PIN_W = 3;
	const PIN_H = 6;

	let color = $derived(handleColor(sourceHandleId));
	let control = $derived(signalOf(sourceHandleId) === 'control');

	let path = $derived.by(() => {
		const p = { sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition };
		switch (edgeSettings.shape) {
			case 'straight':
				return getStraightPath(p)[0];
			case 'step':
				return getSmoothStepPath({ ...p, borderRadius: 0 })[0];
			case 'smoothstep':
				return getSmoothStepPath(p)[0];
			default:
				return getBezierPath(p)[0];
		}
	});
</script>

<!-- BaseEdge stays underneath for xyflow's hit area and selection styling. -->
<BaseEdge {id} {path} style="stroke:transparent;stroke-width:6px" {markerEnd} />
<g class="pointer-events-none" opacity={selected ? 0.4 : 1}>
	<path d={path} fill="none" stroke={color} stroke-width="2" stroke-dasharray={control ? CONTROL_DASH : undefined} />
	{#if edgeSettings.pins}
		{#each [[sourceX, sourceY], [targetX, targetY]] as [x, y], end (end)}
			{#if control}
				<path
					d={`M${x} ${y - PIN_H / 2}L${x + PIN_H / 2} ${y}L${x} ${y + PIN_H / 2}L${x - PIN_H / 2} ${y}Z`}
					fill={color}
					stroke={darken(color)}
					stroke-width="0.5" />
			{:else}
				<rect x={x - PIN_W / 2} y={y - PIN_H / 2} width={PIN_W} height={PIN_H} rx="1" fill={color} stroke={darken(color)} stroke-width="0.5" />
			{/if}
		{/each}
	{/if}
</g>
