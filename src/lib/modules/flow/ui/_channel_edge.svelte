<script lang="ts">
	import { BaseEdge, getBezierPath, getSmoothStepPath, getStraightPath, type EdgeProps } from '@xyflow/svelte';
	import { CONTROL_DASH, darken, handleColor, signalOf } from '$lib/modules/flow/utils';
	import { edgeSettings } from '../edge_settings.svelte';
	import { controlSignals } from '../control.svelte';

	let { id, source, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, sourceHandleId, selected, markerEnd }: EdgeProps = $props();

	const PIN_W = 3;
	const PIN_H = 6;

	let color = $derived(handleColor(sourceHandleId));
	let control = $derived(signalOf(sourceHandleId) === 'control');
	let live = $derived(control && !!sourceHandleId && controlSignals.read(source, sourceHandleId));

	// `steady` until the signal first moves, so a cable mounted live or idle
	// does not play an edge it never saw.
	let phase = $state<'steady' | 'rise' | 'fall'>('steady');
	let wasLive: boolean | null = null;
	$effect.pre(() => {
		const now = live;
		if (wasLive !== null && now !== wasLive) phase = now ? 'rise' : 'fall';
		wasLive = now;
	});

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
	{#if control}
		<!-- pathLength 1 lets one dash cover the whole cable, whatever its length. -->
		{#key phase}
			<path d={path} pathLength="1" fill="none" stroke-width="2" class={['charge', phase, live && 'live']} />
		{/key}
		{#if live}
			<path d={path} pathLength="1" fill="none" stroke-width="2" class="flow" />
		{/if}
	{/if}
	{#if edgeSettings.pins}
		{#each [[sourceX, sourceY], [targetX, targetY]] as [x, y], end (end)}
			{#if control}
				<path
					d={`M${x} ${y - 4}L${x + 4} ${y}L${x} ${y + 4}L${x - 4} ${y}Z`}
					class={live ? 'pin-live' : 'pin-idle'}
					stroke-width="1.5"
					stroke-linejoin="round" />
			{:else}
				<rect x={x - PIN_W / 2} y={y - PIN_H / 2} width={PIN_W} height={PIN_H} rx="1" fill={color} stroke={darken(color)} stroke-width="0.5" />
			{/if}
		{/each}
	{/if}
</g>

<style>
	/* One dash the length of the cable, slid in from the source when the
	   signal rises and out past the target when it falls. */
	.charge {
		stroke: var(--color-signal);
		stroke-linecap: round;
		stroke-dasharray: 1 1;
		stroke-dashoffset: 1;
		filter: drop-shadow(0 0 3px var(--color-signal));
	}
	.charge.steady.live {
		stroke-dashoffset: 0;
	}
	.charge.rise {
		animation: rise 320ms ease-out forwards;
	}
	.charge.fall {
		animation: fall 320ms ease-in forwards;
	}
	@keyframes rise {
		from {
			stroke-dashoffset: 1;
		}
		to {
			stroke-dashoffset: 0;
		}
	}
	@keyframes fall {
		from {
			stroke-dashoffset: 0;
		}
		to {
			stroke-dashoffset: -1;
		}
	}
	/* Brighter ripples riding the lit cable toward the target. */
	.flow {
		stroke: color-mix(in oklch, var(--color-signal) 40%, white);
		stroke-linecap: round;
		stroke-dasharray: 0.03 0.09;
		animation: flow 1.2s linear infinite;
		opacity: 0.9;
	}
	@keyframes flow {
		to {
			stroke-dashoffset: -0.12;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.charge.rise,
		.charge.fall {
			animation-duration: 1ms;
		}
		.flow {
			display: none;
		}
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
