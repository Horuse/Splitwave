<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { type Node, type NodeProps } from '@xyflow/svelte';
	import type { LevelMeterNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { onNodeAction } from '$lib/modules/flow/utils';

	type LevelMeterNodeType = Node<LevelMeterNodeData, 'levelMeter'>;
	let { id }: NodeProps<LevelMeterNodeType> = $props();

	let targetPeakL = 0;
	let targetPeakR = 0;
	let targetRmsL = 0;
	let targetRmsR = 0;

	let displayPeakL = $state(-Infinity);
	let displayPeakR = $state(-Infinity);
	let displayRmsL = $state(-Infinity);
	let displayRmsR = $state(-Infinity);

	let holdPeakL = $state(-Infinity);
	let holdPeakR = $state(-Infinity);
	let holdTimeL = 0;
	let holdTimeR = 0;

	let maxPeakL = $state(-Infinity);
	let maxPeakR = $state(-Infinity);

	let clipL = $state(false);
	let clipR = $state(false);

	// Hover tooltip
	let hoverDb = $state<number | null>(null);
	let hoverY = $state(0);

	const DB_FLOOR = -60;
	const PEAK_FALL_DB_PER_SEC = 20;
	const HOLD_TIME_MS = 1500;
	const HOLD_FALL_DB_PER_SEC = 20;

	const dbLabels = Array.from({ length: 20 }, (_, i) => -i * 3);
	const minorTicks = Array.from({ length: 60 }, (_, i) => -i);

	const minorTickPos = minorTicks.map((db) => ({ db, pct: dbToPct(db), major: db % 3 === 0 }));
	const dbLabelPos = dbLabels.map((db) => ({ db, pct: dbToPct(db) }));

	const METER_GRADIENT = `linear-gradient(to top,
        #22c55e 0%, #22c55e 70%,
        #eab308 70%, #eab308 90%,
        #f97316 90%, #f97316 95%,
        #ef4444 95%, #ef4444 100%)`;

	interface MeterTick {
		nodeId: string;
		peakL: number;
		peakR: number;
		rmsL: number;
		rmsR: number;
	}

	function ampToDb(amp: number): number {
		if (amp <= 1e-6) return -Infinity;
		return 20 * Math.log10(amp);
	}

	function dbToPct(db: number): number {
		if (!isFinite(db)) return 0;
		return Math.max(0, Math.min(100, ((db - DB_FLOOR) / -DB_FLOOR) * 100));
	}

	function pctToDb(pct: number): number {
		return (pct / 100) * -DB_FLOOR + DB_FLOOR;
	}

	function formatDb(db: number): string {
		return isFinite(db) ? db.toFixed(1) : '−∞';
	}

	// Max readout background color, based on the highest peak ever reached
	function maxBgClass(maxL: number, maxR: number): string {
		const m = Math.max(maxL, maxR);
		if (!isFinite(m)) return 'bg-neutral-200';
		if (m >= -1) return 'bg-red-300';
		if (m >= -6) return 'bg-amber-200';
		return 'bg-neutral-200';
	}

	function handleBarHover(e: MouseEvent) {
		const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
		const y = e.clientY - rect.top;
		const pct = Math.max(0, Math.min(100, 100 - (y / rect.height) * 100));
		hoverDb = pctToDb(pct);
		hoverY = y;
	}

	function clearHover() {
		hoverDb = null;
	}

	let unlisten: UnlistenFn | undefined;
	let rafId: number | undefined;
	let lastFrame = 0;

	function tick(now: number) {
		const dt = lastFrame ? Math.min((now - lastFrame) / 1000, 0.1) : 0;
		lastFrame = now;

		const tPeakL = ampToDb(targetPeakL);
		const tPeakR = ampToDb(targetPeakR);
		const tRmsL = ampToDb(targetRmsL);
		const tRmsR = ampToDb(targetRmsR);

		const nextPeakL = tPeakL > displayPeakL
			? tPeakL : Math.max(tPeakL, displayPeakL - PEAK_FALL_DB_PER_SEC * dt);
		if (nextPeakL !== displayPeakL) displayPeakL = nextPeakL;

		const nextPeakR = tPeakR > displayPeakR
			? tPeakR : Math.max(tPeakR, displayPeakR - PEAK_FALL_DB_PER_SEC * dt);
		if (nextPeakR !== displayPeakR) displayPeakR = nextPeakR;

		const nextRmsL = tRmsL > displayRmsL
			? tRmsL : Math.max(tRmsL, displayRmsL - PEAK_FALL_DB_PER_SEC * dt);
		if (nextRmsL !== displayRmsL) displayRmsL = nextRmsL;

		const nextRmsR = tRmsR > displayRmsR
			? tRmsR : Math.max(tRmsR, displayRmsR - PEAK_FALL_DB_PER_SEC * dt);
		if (nextRmsR !== displayRmsR) displayRmsR = nextRmsR;

		if (tPeakL > holdPeakL) {
			holdPeakL = tPeakL;
			holdTimeL = now;
		} else if (now - holdTimeL > HOLD_TIME_MS) {
			const next = Math.max(tPeakL, holdPeakL - HOLD_FALL_DB_PER_SEC * dt);
			if (next !== holdPeakL) holdPeakL = next;
		}
		if (tPeakR > holdPeakR) {
			holdPeakR = tPeakR;
			holdTimeR = now;
		} else if (now - holdTimeR > HOLD_TIME_MS) {
			const next = Math.max(tPeakR, holdPeakR - HOLD_FALL_DB_PER_SEC * dt);
			if (next !== holdPeakR) holdPeakR = next;
		}

		if (tPeakL > maxPeakL) maxPeakL = tPeakL;
		if (tPeakR > maxPeakR) maxPeakR = tPeakR;

		if (targetPeakL >= 1.0 && !clipL) clipL = true;
		if (targetPeakR >= 1.0 && !clipR) clipR = true;

		rafId = requestAnimationFrame(tick);
	}

	let unlistenReset: (() => void) | undefined;
	onMount(async () => {
		unlistenReset = onNodeAction(id, 'resetPeaks', () => resetPeaks());
		unlisten = await listen<MeterTick>('audio://meter', (event) => {
			const p = event.payload;
			if (p.nodeId !== id) return;
			targetPeakL = p.peakL;
			targetPeakR = p.peakR;
			targetRmsL = p.rmsL;
			targetRmsR = p.rmsR;
		});
		rafId = requestAnimationFrame(tick);
	});

	onDestroy(() => {
		unlisten?.();
		unlistenReset?.();
		if (rafId) cancelAnimationFrame(rafId);
	});

	function resetPeaks() {
		holdPeakL = -Infinity;
		holdPeakR = -Infinity;
		maxPeakL = -Infinity;
		maxPeakR = -Infinity;
		clipL = false;
		clipR = false;
	}

	function handleBarKey(e: KeyboardEvent) {
		if (e.key === 'Enter' || e.key === ' ' || e.key === 'Escape') {
			e.preventDefault();
			resetPeaks();
		}
	}
</script>

<Wrapper label="Level Meter" accent="effect" hasInput hasOutput>
	<div class="flex w-fit flex-col gap-1">
		<div class="flex gap-1.5">
			<div class="flex flex-col gap-0.5">
				<!-- Clip LEDs -->
				<div class="flex h-2 w-16 overflow-hidden rounded-sm border border-neutral-300">
					{#each [{ c: clipL, lab: 'L' }, { c: clipR, lab: 'R' }] as item, i (i)}
						<button
							type="button"
							class="flex-1 transition-colors {item.c
                         ? 'bg-red-600 shadow-[inset_0_0_4px_#fca5a5]'
                         : 'bg-neutral-200'} {i === 0 ? 'border-r border-neutral-300' : ''}"
							onclick={resetPeaks}
							aria-label="Clip {item.lab} (click to reset)"
						></button>
					{/each}
				</div>

				<!-- Meter with hover and click-to-reset -->
				<div
					class="relative flex h-72 w-16 cursor-crosshair overflow-hidden rounded-sm border border-neutral-300"
					style="--bar-h: 288px;"
					onmousemove={handleBarHover}
					onmouseleave={clearHover}
					onclick={resetPeaks}
					onkeydown={handleBarKey}
					role="button"
					tabindex="0"
					aria-label="Level meter — click to reset peaks, hover to read level"
				>
					{#each [{ p: displayPeakL, r: displayRmsL, h: holdPeakL }, { p: displayPeakR, r: displayRmsR, h: holdPeakR }] as ch, i (i)}
						<div class="relative flex-1 {i === 0 ? 'border-r border-neutral-300' : ''}">
							<div
								class="absolute inset-0 opacity-30 dark:brightness-[0.2]"
								style="background: {METER_GRADIENT};"
							></div>
							<div
								class="absolute right-0 bottom-0 left-0"
								style="height: {dbToPct(ch.p)}%;
                                background: {METER_GRADIENT};
                                background-size: 100% var(--bar-h);
                                background-position: bottom;
                                background-repeat: no-repeat;"
							></div>
							<div
								class="absolute right-0 left-0 h-px bg-white/80 mix-blend-overlay"
								style="bottom: {dbToPct(ch.r)}%;"
							></div>
							{#if isFinite(ch.h) && ch.h > DB_FLOOR}
								<div
									class="absolute right-0 left-0 h-0.5 bg-white shadow-[0_0_2px_white]"
									style="bottom: calc({dbToPct(ch.h)}% - 1px);"
								></div>
							{/if}
						</div>
					{/each}

					<!-- Hover line + dB badge -->
					{#if hoverDb !== null}
						<div
							class="pointer-events-none absolute right-0 left-0 z-10 h-px bg-cyan-400"
							style="top: {hoverY}px;"
						>
                      <span
						  class="absolute left-1/2 -translate-x-1/2 whitespace-nowrap rounded bg-neutral-800 px-1 font-mono text-[8px] leading-tight text-white"
	                      style="top: {hoverY < 12 ? '2px' : '-10px'};"
					  >
                         {hoverDb.toFixed(1)}
                      </span>
						</div>
					{/if}
				</div>
			</div>

			<!-- dB scale -->
			<div
				class="relative h-72 w-8 font-mono text-[9px] text-neutral-900 select-none"
				style="margin-top: 10px;"
			>
				{#each minorTickPos as t (t.db)}
					<div
						class="absolute left-0 h-px bg-neutral-700 {t.major ? 'w-2' : 'w-1'}"
						style="bottom: {t.pct}%;"
					></div>
				{/each}
				{#each dbLabelPos as l (l.db)}
					<div
						class="absolute left-2.5 -translate-y-1/2 leading-none"
						style="bottom: {l.pct}%;"
					>
						{l.db}
					</div>
				{/each}
				<div class="absolute bottom-0 left-2.5 leading-none">dB</div>
			</div>
		</div>

		<!-- Max readout; background tints when the max has been hot -->
		<button
			type="button"
			onclick={resetPeaks}
			title="Reset peaks"
			class="flex w-16 flex-col rounded-sm border border-neutral-300 px-1 py-0.5 font-mono text-[9px] text-neutral-900 transition-colors hover:opacity-80 {maxBgClass(maxPeakL, maxPeakR)}"
		>
			<div class="flex justify-between">
				<span class="opacity-60">L</span>
				<span>{formatDb(maxPeakL)}</span>
			</div>
			<div class="flex justify-between">
				<span class="opacity-60">R</span>
				<span>{formatDb(maxPeakR)}</span>
			</div>
		</button>

		<!-- Solo -->
		<div class="flex w-16 overflow-hidden rounded-sm border border-neutral-300">
			{#each ['L', 'R'] as ch, i (ch)}
				<button
					type="button"
					aria-label="Solo {ch}"
					class="flex h-4 flex-1 items-center justify-center bg-neutral-200 font-mono text-[9px] text-neutral-900 hover:bg-neutral-300 {i === 0
                   ? 'border-r border-neutral-300'
                   : ''}"
				>
					S
				</button>
			{/each}
		</div>
	</div>
</Wrapper>