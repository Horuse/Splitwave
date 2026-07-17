<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { Handle, Position } from '@xyflow/svelte';
	import { channelColor, channelLabel } from '$lib/modules/flow/utils';

	let {
		nodeId,
		channelCount = 0,
		split = false
	}: { nodeId: string; channelCount?: number; split?: boolean } = $props();

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

	// Row count follows the real channel count so the layout is stable before
	// meter data arrives; falls back to whatever the last tick delivered.
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

<div class="grid w-full grid-cols-[minmax(2px,max-content)_1fr] items-center gap-x-1.5 gap-y-1.5" aria-label="Live input level">
	{#each rows as i (i)}
		{@const db = displays[i] ?? -Infinity}
		{@const hold = holds[i] ?? -Infinity}
		<span
			class="text-right font-mono text-[8px] leading-none"
			style="color:{channelColor(i)}">{channelLabel(i, rows.length)}</span
		>
		<div class={['relative flex items-center', split && '-mr-4 pr-4']}>
			<div class="relative h-2 flex-1 overflow-hidden rounded-sm bg-neutral-300">
				<div
					class="absolute inset-0"
					style="
                   background: linear-gradient(to right, #22c55e 0%, #22c55e 70%, #eab308 70%, #eab308 90%, #f97316 90%, #f97316 95%, #ef4444 95%, #ef4444 100%);
                   clip-path: inset(0 {100 - dbToPct(db)}% 0 0);
                "
				></div>
				{#if isFinite(hold) && dbToPct(hold) > 0}
					<div class="absolute inset-y-0 w-px bg-white" style="left: {dbToPct(hold)}%;"></div>
				{/if}
			</div>
			{#if split}
				<Handle
					type="source"
					id={`ch${i + 1}`}
					position={Position.Right}
					class="handle"
					style={`background:${channelColor(i)}`}
				/>
			{/if}
		</div>
	{/each}
</div>