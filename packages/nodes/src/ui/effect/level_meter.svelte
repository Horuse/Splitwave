<script lang="ts">
	import { MeterBar } from '@splitwave/ui';
	import { onDestroy, onMount } from 'svelte';
	import { getEnginePort } from '../../engine';
	import { type Node, type NodeProps } from '@xyflow/svelte';
	import type { LevelMeterNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import DbScale from '../graph/db_scale.svelte';
	import DataBar from '~icons/fluent/data-bar-vertical-16-filled';
	import { onNodeAction, channelColor, channelLabel } from '../utils';
	import { formatDb } from '@splitwave/nodes';

	type LevelMeterNodeType = Node<LevelMeterNodeData, 'levelMeter'>;
	let { id, data }: NodeProps<LevelMeterNodeType> = $props();

	const DB_FLOOR = -60;
	const PEAK_FALL_DB_PER_SEC = 20;
	const HOLD_TIME_MS = 1500;
	const HOLD_FALL_DB_PER_SEC = 20;
	const BAR_W = 30;


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

	// With nothing wired in the tick arrays are empty; fall back to a single
	// placeholder channel so the bar tracks still render their ghost background.
	let barVals = $derived(displayPeaks.length ? displayPeaks : [-Infinity]);
	let clipVals = $derived(clips.length ? clips : [false]);
	let maxVals = $derived(maxPeaks.length ? maxPeaks : [-Infinity]);
	let channelCount = $derived(barVals.length);

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

	function hoverLabel(pct: number): string {
		return pctToDb(pct).toFixed(1);
	}

	function dbTextClass(db: number): string {
		if (!isFinite(db) || db <= DB_FLOOR) return 'text-neutral-500';
		if (db >= -1) return 'text-red-500';
		if (db >= -6) return 'text-amber-500';
		return 'text-neutral-900';
	}

	let unlistenReset: (() => void) | undefined;
	let rafId: number | undefined;
	let lastFrame = 0;

	function fall(target: number, current: number, dt: number): number {
		return target > current ? target : Math.max(target, DB_FLOOR, current - PEAK_FALL_DB_PER_SEC * dt);
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

	const engine = getEnginePort();
	unlistenReset = onNodeAction(id, 'resetPeaks', () => resetPeaks());
	engine.listen<MeterTick>('audio://meter', (p) => {
		if (p.nodeId !== id) return;
		targetPeaks = p.peaks;
		targetRms = p.rms;
	});

	onMount(() => {
		rafId = requestAnimationFrame(tick);
	});

	onDestroy(() => {
		unlistenReset?.();
		if (rafId) cancelAnimationFrame(rafId);
	});
</script>

<Wrapper label="Level Meter" accent="monitor" icon={DataBar} hasInput hasOutput channelIo nodeId={id} wide>
	<div class="flex gap-2">
		<div class="flex flex-col gap-1.5" style="width: {channelCount * BAR_W}px;">
			<!-- Clip lamps: click to reset peaks. -->
			<div class="flex gap-1">
				{#each clipVals as c, i (i)}
					<button
						type="button"
						class={['h-1.5 flex-1 rounded-full', c ? 'bg-red-500' : 'bg-neutral-300']}
						onclick={resetPeaks}
						aria-label="Clip {channelLabel(i, channelCount)} (click to reset)"></button>
				{/each}
			</div>

			<div
				class="flex h-72 cursor-crosshair gap-1"
				onclick={resetPeaks}
				onkeydown={handleBarKey}
				role="button"
				tabindex="0"
				aria-label="Level meter: click to reset peaks, hover to read level">
				{#each barVals as p, i (i)}
					<MeterBar class="flex-1 overflow-hidden rounded-sm" orientation="vertical" gradient={METER_GRADIENT} ghost hover {hoverLabel} pct={dbToPct(p)}>
						<div class="absolute right-0 left-0 h-px bg-white/70" style="bottom: {dbToPct(displayRms[i] ?? -Infinity)}%;"></div>
						{#if isFinite(holdPeaks[i]) && holdPeaks[i] > DB_FLOOR}
							<div class="absolute right-0 left-0 h-0.5 bg-white" style="bottom: calc({dbToPct(holdPeaks[i])}% - 1px);"></div>
						{/if}
					</MeterBar>
				{/each}
			</div>

			<!-- Live level and max per channel, each under its bar; click a max to reset peaks. -->
			<div class="grid gap-x-1 gap-y-0.5 font-mono text-[9px] tabular-nums" style="grid-template-columns: repeat({channelCount}, minmax(0, 1fr));">
				{#each barVals as _, i (i)}
					<span class="text-center leading-none" style="color: {channelColor(i)}">{channelLabel(i, channelCount)}</span>
				{/each}
				{#each barVals as db, i (i)}
					<span class={['text-center leading-tight', dbTextClass(db)]}>{formatDb(db, DB_FLOOR)}</span>
				{/each}
				{#each maxVals as db, i (i)}
					<button
						type="button"
						onclick={resetPeaks}
						title="Reset peaks"
						class={['rounded-sm bg-(--lv-1) text-center leading-tight hover:bg-(--lv-2)', dbTextClass(db)]}>
						{formatDb(db, DB_FLOOR)}
					</button>
				{/each}
			</div>
		</div>

		<!-- dB scale, level with the bars under the clip row. -->
		<div class="mt-3"><DbScale min={DB_FLOOR} height={288} /></div>
	</div>
</Wrapper>
