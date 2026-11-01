<script lang="ts">
	import { BaseEdge, getBezierPath, getSmoothStepPath, getStraightPath, type EdgeProps } from '@xyflow/svelte';
	import { prefersReducedMotion } from 'svelte/motion';
	import { CONTROL_DASH, darken, handleColor, signalOf } from '$lib/modules/flow/utils';
	import { edgeSettings } from '../edge_settings.svelte';
	import { controlSignals } from '../control.svelte';

	let { id, source, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, sourceHandleId, selected, markerEnd }: EdgeProps = $props();

	const PIN_W = 3;
	const PIN_H = 6;
	const CHARGE_MS = 320;

	let color = $derived(handleColor(sourceHandleId));
	let control = $derived(signalOf(sourceHandleId) === 'control');
	let live = $derived(control && !!sourceHandleId && controlSignals.read(source, sourceHandleId));

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

	// The charge is one dash as long as the cable: it slides in from the source
	// when the signal rises and out past the target when it falls. Lengths are
	// measured in pixels; WebKit ignores `pathLength` for dashes.
	let charge = $state<SVGPathElement>();
	let draining = $state(false);
	let rising = false;
	let wasLive: boolean | null = null;
	$effect.pre(() => {
		const now = live;
		if (wasLive === true && !now) draining = true;
		if (wasLive === false && now) {
			rising = true;
			draining = false;
		}
		wasLive = now;
	});

	$effect(() => {
		const el = charge;
		path;
		if (!el) return;
		const len = el.getTotalLength();
		el.style.strokeDasharray = `${len} ${len}`;
		const duration = prefersReducedMotion.current ? 1 : CHARGE_MS;
		if (draining) {
			const run = el.animate([{ strokeDashoffset: 0 }, { strokeDashoffset: -len }], { duration, easing: 'ease-in', fill: 'forwards' });
			run.onfinish = () => (draining = false);
			return () => run.cancel();
		}
		if (rising) {
			rising = false;
			const run = el.animate([{ strokeDashoffset: len }, { strokeDashoffset: 0 }], { duration, easing: 'ease-out', fill: 'forwards' });
			return () => run.cancel();
		}
		el.style.strokeDashoffset = '0';
	});
</script>

<!-- BaseEdge stays underneath for xyflow's hit area and selection styling. -->
<BaseEdge {id} {path} style="stroke:transparent;stroke-width:6px" {markerEnd} />
<g class="pointer-events-none" opacity={selected ? 0.4 : 1}>
	<path d={path} fill="none" stroke={color} stroke-width="2" stroke-dasharray={control ? CONTROL_DASH : undefined} />
	{#if live || draining}
		<path bind:this={charge} d={path} fill="none" stroke-width="2" class="charge" />
	{/if}
	{#if live && !prefersReducedMotion.current}
		<path d={path} fill="none" stroke-width="2" class="flow" />
	{/if}
	<!-- A control socket is drawn by its node; only audio cables carry pins. -->
	{#if edgeSettings.pins && !control}
		{#each [[sourceX, sourceY], [targetX, targetY]] as [x, y], end (end)}
			<rect x={x - PIN_W / 2} y={y - PIN_H / 2} width={PIN_W} height={PIN_H} rx="1" fill={color} stroke={darken(color)} stroke-width="0.5" />
		{/each}
	{/if}
</g>

<style>
	.charge {
		stroke: var(--color-signal);
		filter: drop-shadow(0 0 3px var(--color-signal));
	}
	/* Brighter ripples riding the lit cable toward the target. */
	.flow {
		stroke: color-mix(in oklch, var(--color-signal) 40%, white);
		stroke-dasharray: 3 11;
		animation: flow 0.7s linear infinite;
		opacity: 0.85;
	}
	@keyframes flow {
		to {
			stroke-dashoffset: -14;
		}
	}
</style>
