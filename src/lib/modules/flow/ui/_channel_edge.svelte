<script lang="ts">
	import { BaseEdge, getBezierPath, getSmoothStepPath, getStraightPath, type EdgeProps } from '@xyflow/svelte';
	import { untrack } from 'svelte';
	import { prefersReducedMotion } from 'svelte/motion';
	import { CONTROL_DASH, darken, handleColor, signalOf } from '$lib/modules/flow/utils';
	import { edgeSettings } from '../edge_settings.svelte';
	import { controlSignals } from '../control.svelte';
	import { cableLevel } from '../levels.svelte';

	let { id, source, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, sourceHandleId, selected, markerEnd }: EdgeProps = $props();

	const PIN_W = 3;
	const PIN_H = 6;
	const CHARGE_MS = 320;

	let color = $derived(handleColor(sourceHandleId));
	let control = $derived(signalOf(sourceHandleId) === 'control');
	let live = $derived(control && !!sourceHandleId && controlSignals.read(source, sourceHandleId));
	let level = $derived(control ? 0 : cableLevel(source, sourceHandleId));

	// The cable's recent levels, newest at the source: each meter tick enters
	// there and the older ones ride toward the target, so the audio is seen
	// travelling along the cable as a waveform envelope.
	const SAMPLES = 96;
	const MAX_SWING = 6;
	let history = $state<number[]>(Array(SAMPLES).fill(0));
	$effect(() => {
		const l = level;
		untrack(() => (history = [l, ...history.slice(0, SAMPLES - 1)]));
	});
	let audible = $derived(history.some((l) => l > 0));

	let wire = $state<SVGPathElement>();
	let envelope = $state('');
	$effect(() => {
		const el = wire;
		path;
		const h = history;
		if (!el || control || !audible) {
			envelope = '';
			return;
		}
		const len = el.getTotalLength();
		const points: string[] = [];
		for (let i = 0; i < SAMPLES; i++) {
			const at = (i / (SAMPLES - 1)) * len;
			const p = el.getPointAtLength(at);
			const q = el.getPointAtLength(Math.min(len, at + 1));
			const r = el.getPointAtLength(Math.max(0, at - 1));
			const dx = q.x - r.x;
			const dy = q.y - r.y;
			const n = Math.hypot(dx, dy) || 1;
			// Alternate sides each sample: the trace swings across the cable
			// the way a waveform crosses zero, as far as the level reaches.
			const swing = (i % 2 === 0 ? 1 : -1) * h[i] * MAX_SWING;
			points.push(`${p.x - (dy / n) * swing} ${p.y + (dx / n) * swing}`);
		}
		envelope = `M${points.join('L')}`;
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
	{#if envelope}
		<path d={envelope} fill="none" stroke={color} stroke-width="1.5" stroke-linejoin="round" class="envelope" />
	{/if}
	<path bind:this={wire} d={path} fill="none" stroke={color} stroke-width="2" stroke-dasharray={control ? CONTROL_DASH : undefined} />
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
	.envelope {
		opacity: 0.85;
	}
</style>
