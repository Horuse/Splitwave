<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { type Node, type NodeProps } from '@xyflow/svelte';
	import type { LevelMeterNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { DataBar } from '$lib/components/icons';
	import MeterBar from '$lib/components/meter_bar.svelte';
	import { onNodeAction, channelColor, channelLabel } from '$lib/modules/flow/utils';

	type LevelMeterNodeType = Node<LevelMeterNodeData, 'levelMeter'>;
	let { id, data }: NodeProps<LevelMeterNodeType> = $props();

	const DB_FLOOR = -60;
	const PEAK_FALL_DB_PER_SEC = 20;
	const HOLD_TIME_MS = 1500;
	const HOLD_FALL_DB_PER_SEC = 20;
	const BAR_W = 30;

	const minorTicks = Array.from({ length: 60 }, (_, i) => -i);
	const minorTickPos = minorTicks.map((db) => ({ db, pct: dbToPct(db), major: db % 3 === 0 }));

	const METER_GRADIENT = `linear-gradient(to top,
        #22c55e 0%, #22c55e 70%,
        #eab308 70%, #eab308 90%,
        #f97316 90%, #f97316 95%,
        #ef4444 95%, #ef4444 100%)`;

	interface MeterTick {
		nodeId: string;
		peaks: number[];
		rms: number[];
	}

	let targetPeaks: number[] = [];
	let targetRms: number[] = [];

	let displayPeaks = $state<number[]>([]);
	let displayRms = $state<number[]>([]);
	let holdPeaks = $state<number[]>([]);
	let holdTimes: number[] = [];
	let maxPeaks = $state<number[]>([]);
	let clips = $state<boolean[]>([]);

	let channelCount = $derived(Math.max(displayPeaks.length, 1));

	function ampToDb(amp: number): number {
		return amp <= 1e-6 ? -Infinity : 20 * Math.log10(amp);
	}

	function dbToPct(db: number): number {
		if (!isFinite(db)) return 0;
		return Math.max(0, Math.min(100, ((db - DB_FLOOR) / -DB_FLOOR) * 100));
	}

	function pctToDb(pct: number): number {
		return (pct / 100) * -DB_FLOOR + DB_FLOOR;
	}

	function formatDb(db: number): string {
		return isFinite(db) && db > DB_FLOOR ? db.toFixed(1) : '−∞';
	}

	function hoverLabel(pct: number): string {
		return pctToDb(pct).toFixed(1);
	}

	function dbTextClass(db: number): string {
		if (!isFinite(db) || db <= DB_FLOOR) return 'text-neutral-400';
		if (db >= -1) return 'text-red-500';
		if (db >= -6) return 'text-amber-500';
		return 'text-neutral-700';
	}

	let unlisten: UnlistenFn | undefined;
	let unlistenReset: (() => void) | undefined;
	let rafId: number | undefined;
	let lastFrame = 0;

	function fall(target: number, current: number, dt: number): number {
		return target > current
			? target
			: Math.max(target, DB_FLOOR, current - PEAK_FALL_DB_PER_SEC * dt);
	}

	function tick(now: number) {
		const dt = lastFrame ? Math.min((now - lastFrame) / 1000, 0.1) : 0;
		lastFrame = now;
		const n = targetPeaks.length;
		const nextPeaks = new Array(n);
		const nextRms = new Array(n);
		const nextHolds = new Array(n);
		const nextMax = new Array(n);
		const nextClips = new Array(n);
		for (let i = 0; i < n; i++) {
			const tp = ampToDb(targetPeaks[i]);
			const tr = ampToDb(targetRms[i] ?? 0);
			nextPeaks[i] = fall(tp, displayPeaks[i] ?? -Infinity, dt);
			nextRms[i] = fall(tr, displayRms[i] ?? -Infinity, dt);
			const h = holdPeaks[i] ?? -Infinity;
			if (tp > h) {
				nextHolds[i] = tp;
				holdTimes[i] = now;
			} else if (now - (holdTimes[i] ?? 0) > HOLD_TIME_MS) {
				nextHolds[i] = Math.max(tp, h - HOLD_FALL_DB_PER_SEC * dt);
			} else {
				nextHolds[i] = h;
			}
			nextMax[i] = Math.max(maxPeaks[i] ?? -Infinity, tp);
			nextClips[i] = (clips[i] ?? false) || targetPeaks[i] >= 1.0;
		}
		displayPeaks = nextPeaks;
		displayRms = nextRms;
		holdPeaks = nextHolds;
		maxPeaks = nextMax;
		clips = nextClips;
		rafId = requestAnimationFrame(tick);
	}

	function resetPeaks() {
		holdPeaks = holdPeaks.map(() => -Infinity);
		maxPeaks = maxPeaks.map(() => -Infinity);
		clips = clips.map(() => false);
	}

	function handleBarKey(e: KeyboardEvent) {
		if (e.key === 'Enter' || e.key === ' ' || e.key === 'Escape') {
			e.preventDefault();
			resetPeaks();
		}
	}

	onMount(async () => {
		unlistenReset = onNodeAction(id, 'resetPeaks', () => resetPeaks());
		unlisten = await listen<MeterTick>('audio://meter', (event) => {
			const p = event.payload;
			if (p.nodeId !== id) return;
			targetPeaks = p.peaks;
			targetRms = p.rms;
		});
		rafId = requestAnimationFrame(tick);
	});

	onDestroy(() => {
		unlisten?.();
		unlistenReset?.();
		if (rafId) cancelAnimationFrame(rafId);
	});
</script>

<Wrapper
	label="Level Meter"
	accent="effect"
	icon={DataBar}
	hasInput
	hasOutput
	channelIo
	nodeId={id}
>
	<div class="flex w-fit flex-col gap-1">
		<div class="flex gap-1.5">
			<div class="flex flex-col gap-0.5">
				<!-- Clip row -->
				<div class="flex h-2 overflow-hidden rounded-sm border border-neutral-300" style="width: {channelCount * BAR_W}px;">
					{#each clips as c, i (i)}
						<button
							type="button"
							class="flex-1 transition-colors {c ? 'bg-red-600 shadow-[inset_0_0_4px_#fca5a5]' : 'bg-neutral-200'} {i > 0 ? 'border-l border-neutral-300' : ''}"
							onclick={resetPeaks}
							aria-label="Clip {channelLabel(i, channelCount)} (click to reset)"
						></button>
					{/each}
				</div>

				<!-- Bars -->
				<div
					class="relative flex h-72 cursor-crosshair overflow-hidden rounded-sm border border-neutral-300"
					style="width: {channelCount * BAR_W}px; --bar-h: 288px;"
					onclick={resetPeaks}
					onkeydown={handleBarKey}
					role="button"
					tabindex="0"
					aria-label="Level meter — click to reset peaks, hover to read level"
				>
					{#each displayPeaks as p, i (i)}
						<MeterBar
							class="flex-1 {i > 0 ? 'border-l border-neutral-300' : ''}"
							orientation="vertical"
							gradient={METER_GRADIENT}
							ghost
							hover
							{hoverLabel}
							pct={dbToPct(p)}
						>
							<div class="absolute right-0 left-0 h-px bg-white/80 mix-blend-overlay" style="bottom: {dbToPct(displayRms[i] ?? -Infinity)}%;"></div>
							{#if isFinite(holdPeaks[i]) && holdPeaks[i] > DB_FLOOR}
								<div class="absolute right-0 left-0 h-0.5 bg-white shadow-[0_0_2px_white]" style="bottom: calc({dbToPct(holdPeaks[i])}% - 1px);"></div>
							{/if}
						</MeterBar>
					{/each}
				</div>
			</div>

			<!-- dB scale -->
			<div class="relative h-72 w-8 font-mono text-[9px] text-neutral-900 select-none" style="margin-top: 10px;">
				{#each minorTickPos as t (t.db)}
					<div class="absolute left-0 flex items-center" style="bottom: {t.pct}%; height: 1px;">
						<div class="shrink-0 bg-neutral-700 {t.major ? 'w-2' : 'w-1'}" style="height: 1px;"></div>
						{#if t.major}<span class="ml-0.5 mb-px leading-none">{t.db}</span>{/if}
					</div>
				{/each}
				<div class="absolute bottom-0 left-2.5 leading-none">dB</div>
			</div>
		</div>

		<!-- Live dB readout -->
		<div class="flex overflow-hidden rounded-sm border border-neutral-300 bg-neutral-100" style="width: {channelCount * BAR_W}px;">
			{#each displayPeaks as db, i (i)}
				<div class="flex flex-1 flex-col items-center py-0.5 {i > 0 ? 'border-l border-neutral-300' : ''}">
					<span class="text-[7px] leading-none" style="color: {channelColor(i)}">{channelLabel(i, channelCount)}</span>
					<span class="font-mono tabular-nums text-[8px] leading-tight {dbTextClass(db)}">{formatDb(db)}</span>
				</div>
			{/each}
		</div>

		<!-- Max peak readout -->
		<button
			type="button"
			onclick={resetPeaks}
			title="Reset peaks"
			class="flex overflow-hidden rounded-sm border border-neutral-300 bg-neutral-200 transition-colors hover:opacity-80"
			style="width: {channelCount * BAR_W}px;"
		>
			{#each maxPeaks as db, i (i)}
				<div class="flex flex-1 flex-col items-center py-0.5 {i > 0 ? 'border-l border-neutral-300' : ''}">
					<span class="text-[7px] leading-none text-neutral-500">{channelLabel(i, channelCount)}</span>
					<span class="font-mono tabular-nums text-[8px] leading-tight {dbTextClass(db)}">{formatDb(db)}</span>
				</div>
			{/each}
		</button>
	</div>
</Wrapper>
