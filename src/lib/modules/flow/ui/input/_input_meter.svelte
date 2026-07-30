<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { Position } from '@xyflow/svelte';
	import Handle from '../_handle.svelte';
	import { channelColor, channelLabel, handleEdgeStyle } from '$lib/modules/flow/utils';
	import { channelSelection } from '$lib/modules/flow/stores.svelte';
	import MeterBar from '$lib/components/meter_bar.svelte';

	const METER_GRADIENT =
		'linear-gradient(to right, #22c55e 0%, #22c55e 70%, #eab308 70%, #eab308 90%, #f97316 90%, #f97316 95%, #ef4444 95%, #ef4444 100%)';

	let {
		nodeId,
		channelCount = 0,
		side = 'source',

	}: {
		nodeId: string;
		channelCount?: number;
		side?: 'source' | 'target';
	} = $props();

	let isSource = $derived(side === 'source');

	// Capture beats xyflow's mousedown, which would otherwise start a drag.
	function onArm(event: MouseEvent, ch: number) {
		if (!event.altKey || !isSource) return;
		event.stopPropagation();
		event.preventDefault();
		channelSelection.toggle(nodeId, ch);
	}

	const DB_FLOOR = -60;
	const PEAK_FALL_DB_PER_SEC = 30;
	const HOLD_SEC = 1.5;
	const HOLD_FALL_DB_PER_SEC = 20;

	interface MeterTick {
		nodeId: string;
		peaks: number[];
		rms: number[];
	}

	let targets: number[] = [];
	let displays = $state<number[]>([]);
	let holds = $state<number[]>([]);
	let holdTimes: number[] = [];

	let rows = $derived(
		Array.from({ length: Math.max(channelCount, displays.length, 1) }, (_, i) => i)
	);

	function ampToDb(amp: number): number {
		return amp <= 1e-6 ? -Infinity : 20 * Math.log10(amp);
	}

	function dbToPct(db: number): number {
		if (!isFinite(db)) return 0;
		return Math.max(0, Math.min(100, ((db - DB_FLOOR) / -DB_FLOOR) * 100));
	}

	let rafId: number | undefined;
	let unlisten: UnlistenFn | undefined;
	let lastFrame = 0;

	function tick(now: number) {
		const dt = lastFrame ? Math.min((now - lastFrame) / 1000, 0.1) : 0;
		lastFrame = now;
		const n = targets.length;
		const nextDisplays = new Array(n);
		const nextHolds = new Array(n);
		for (let i = 0; i < n; i++) {
			const t = ampToDb(targets[i]);
			const d = displays[i] ?? -Infinity;
			const h = holds[i] ?? -Infinity;
			nextDisplays[i] = t > d ? t : Math.max(t, d - PEAK_FALL_DB_PER_SEC * dt);
			let newHold = h;
			holdTimes[i] = (holdTimes[i] ?? 0) + dt;
			if (t >= h) {
				newHold = t;
				holdTimes[i] = 0;
			} else if (holdTimes[i] > HOLD_SEC) {
				newHold = Math.max(t, h - HOLD_FALL_DB_PER_SEC * dt);
			}
			nextHolds[i] = newHold;
		}
		displays = nextDisplays;
		holds = nextHolds;
		rafId = requestAnimationFrame(tick);
	}

	onMount(async () => {
		unlisten = await listen<MeterTick>('audio://meter', (event) => {
			const p = event.payload;
			if (p.nodeId !== nodeId) return;
			targets = p.peaks;
		});
		rafId = requestAnimationFrame(tick);
	});

	onDestroy(() => {
		unlisten?.();
		if (rafId) cancelAnimationFrame(rafId);
	});
</script>

<div class="flex w-full flex-col gap-1" aria-label="Live level">
	{#each rows as i (i)}
		{@const db = displays[i] ?? -Infinity}
		{@const hold = holds[i] ?? -Infinity}
		<div class={['grid items-center gap-x-1.5', isSource ? 'grid-cols-[minmax(2px,max-content)_1fr]' : 'grid-cols-[1fr_minmax(2px,max-content)]']}>
			{#if isSource}
				<span class="text-right font-mono text-[8px] leading-none" style="color:{channelColor(i)}">
					{channelLabel(i, rows.length)}
				</span>
			{/if}
			<div class="relative flex items-center" onmousedowncapture={(e) => onArm(e, i + 1)}>
				<MeterBar
					class="h-2 flex-1 rounded-sm"
					ghost
					pct={dbToPct(db)}
					gradient={METER_GRADIENT}
					hold={isFinite(hold) ? dbToPct(hold) : null}
				/>
				{#if isSource}
					<div class="wire pointer-events-none absolute top-1/2 -translate-y-1/2" style="right:-1rem; width:1rem; color:{channelColor(i)}"></div>
					<Handle type="source" id={`ch${i + 1}`} position={Position.Right} class={['handle', channelSelection.has(nodeId, i + 1) && 'handle-armed']} style={handleEdgeStyle(channelColor(i), 'source')} />
				{:else}
					<div class="wire pointer-events-none absolute top-1/2 -translate-y-1/2" style="left:-1rem; width:1rem; color:{channelColor(i)}"></div>
					<Handle type="target" id={`ch${i + 1}`} position={Position.Left} class="handle" style={handleEdgeStyle(channelColor(i), 'target')} />
				{/if}
			</div>
			{#if !isSource}
				<span class="text-left font-mono text-[8px] leading-none" style="color:{channelColor(i)}">
					{channelLabel(i, rows.length)}
				</span>
			{/if}
		</div>
	{/each}
</div>
