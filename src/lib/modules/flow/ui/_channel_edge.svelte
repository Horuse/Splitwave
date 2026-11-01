<script lang="ts">
	import { BaseEdge, getBezierPath, getSmoothStepPath, getStraightPath, type EdgeProps } from '@xyflow/svelte';
	import { prefersReducedMotion } from 'svelte/motion';
	import { CONTROL_DASH, darken, handleColor, signalOf } from '$lib/modules/flow/utils';
	import { edgeSettings } from '../edge_settings.svelte';
	import { controlSignals } from '../control.svelte';

	let { id, source, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, sourceHandleId, selected, markerEnd }: EdgeProps = $props();

	const PIN_W = 3;
	const PIN_H = 6;
	// Fireflies keep one pace whatever the cable's length.
	const FIREFLY_PX_PER_S = 160;
	const FIREFLIES = 3;

	let color = $derived(handleColor(sourceHandleId));
	let control = $derived(signalOf(sourceHandleId) === 'control');
	let live = $derived(control && !!sourceHandleId && controlSignals.read(source, sourceHandleId));
	let flightS = $derived(Math.max(0.5, Math.hypot(targetX - sourceX, targetY - sourceY) / FIREFLY_PX_PER_S));

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
	{#if live}
		<path d={path} class="cable-live" fill="none" stroke-width="2" />
		{#if !prefersReducedMotion.current}
			{#each { length: FIREFLIES } as _, i (i)}
				<circle r="2.5" class="firefly">
					<animateMotion dur="{flightS}s" begin="{(-flightS * i) / FIREFLIES}s" repeatCount="indefinite" {path} />
				</circle>
			{/each}
		{/if}
	{:else}
		<path d={path} fill="none" stroke={color} stroke-width="2" stroke-dasharray={control ? CONTROL_DASH : undefined} />
	{/if}
	{#if edgeSettings.pins}
		{#each [[sourceX, sourceY], [targetX, targetY]] as [x, y], end (end)}
			{#if control}
				<circle cx={x} cy={y} r="3" class={live ? 'pin-live' : 'pin-idle'} stroke-width="1.5" />
			{:else}
				<rect x={x - PIN_W / 2} y={y - PIN_H / 2} width={PIN_W} height={PIN_H} rx="1" fill={color} stroke={darken(color)} stroke-width="0.5" />
			{/if}
		{/each}
	{/if}
</g>

<style>
	.cable-live {
		stroke: var(--color-signal);
		filter: drop-shadow(0 0 3px var(--color-signal));
	}
	.firefly {
		fill: white;
		filter: drop-shadow(0 0 2px var(--color-signal)) drop-shadow(0 0 5px var(--color-signal));
	}
	.pin-idle {
		fill: var(--color-neutral-100);
		stroke: var(--color-neutral-750);
	}
	.pin-live {
		fill: var(--color-signal);
		stroke: var(--color-signal);
	}
</style>
