<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { LufsMeterNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { onNodeAction } from '$lib/modules/flow/utils';
	import MeterBar from '$lib/components/meter_bar.svelte';
	import SegmentedButtons from '$lib/components/segmented_buttons.svelte';

	type LufsMeterNodeType = Node<LufsMeterNodeData, 'lufsMeter'>;
	let { id, data }: NodeProps<LufsMeterNodeType> = $props();

	const flow = useSvelteFlow();

	// Sentinel emitted from the engine when LUFS is `-inf` (silent).
	const LUFS_SILENT = -120;

	const LUFS_FLOOR = -40; // bar scale floor

	const BAR_W = 35; // px per channel bar — wide enough for "-15.2"

	const PRESETS: { label: string; subtitle: string; value: number | null }[] = [
		{ label: 'Free', subtitle: '', value: null },
		{ label: '−23', subtitle: 'EBU R128', value: -23 },
		{ label: '−16', subtitle: 'Apple', value: -16 },
		{ label: '−14', subtitle: 'Spotify', value: -14 }
	];

	const LUFS_GRADIENT = `linear-gradient(to top,
        #22c55e 0%, #22c55e 55%,
        #eab308 55%, #eab308 80%,
        #f97316 80%, #f97316 92%,
        #ef4444 92%, #ef4444 100%)`;

	const LUFS_TICKS = [0, -6, -12, -18, -23, -30, -40];

	function setTarget(value: number | null) {
		flow.updateNodeData(id, { target: value });
	}

	function dbToPct(db: number, floor: number): number {
		if (!isFinite(db) || db <= LUFS_SILENT) return 0;
		return Math.max(0, Math.min(100, ((db - floor) / -floor) * 100));
	}

	function hoverLabel(floor: number): (pct: number) => string {
		return (pct) => (floor + (pct / 100) * -floor).toFixed(1);
	}

	let momentary = $state(LUFS_SILENT);
	let shortterm = $state(LUFS_SILENT);
	let integrated = $state(LUFS_SILENT);
	let holdM = $state(LUFS_SILENT);
	let holdS = $state(LUFS_SILENT);
	let holdI = $state(LUFS_SILENT);

	interface LufsTick {
		nodeId: string;
		momentary: number;
		shortterm: number;
		integrated: number;
	}

	function format(v: number): string {
		return v <= LUFS_SILENT ? '−∞' : v.toFixed(1);
	}

	function targetDelta(integrated: number, target: number | null | undefined): string | null {
		if (integrated <= LUFS_SILENT || target == null) return null;
		const d = integrated - target;
		return `${d >= 0 ? '+' : ''}${d.toFixed(1)} LU`;
	}

	function targetClass(integrated: number, target: number | null | undefined): string {
		if (integrated <= LUFS_SILENT) return 'text-neutral-400';
		if (target == null) return 'text-neutral-900';
		const delta = Math.abs(integrated - target);
		if (delta <= 0.5) return 'text-green-600';
		if (delta <= 1.5) return 'text-amber-500';
		return 'text-red-500';
	}

	function resetPeaks() {
		holdM = LUFS_SILENT;
		holdS = LUFS_SILENT;
		holdI = LUFS_SILENT;
	}

	let unlisten: UnlistenFn | undefined;
	let unlistenReset: (() => void) | undefined;

	onMount(async () => {
		unlistenReset = onNodeAction(id, 'resetPeaks', () => resetPeaks());
		unlisten = await listen<LufsTick>('audio://lufs', (event) => {
			const p = event.payload;
			if (p.nodeId !== id) return;
			momentary = p.momentary;
			shortterm = p.shortterm;
			integrated = p.integrated;
			holdM = Math.max(holdM, momentary);
			holdS = Math.max(holdS, shortterm);
			holdI = Math.max(holdI, integrated);
		});
	});

	onDestroy(() => {
		unlisten?.();
		unlistenReset?.();
	});
</script>

<Wrapper label="LUFS Meter" accent="effect" hasInput hasOutput>
	<div class="flex w-fit flex-col gap-1.5 font-mono text-[10px]">
		<div class="flex gap-3">
			<!-- LUFS block: M / S / I -->
			<div class="flex flex-col gap-0.5">
				<div class="flex gap-1.5">
					<button
						type="button"
						onclick={resetPeaks}
						title="Reset LUFS peaks"
						class="nodrag nopan relative flex h-40 items-stretch overflow-hidden rounded-sm border border-neutral-300"
						style="width: {BAR_W * 3}px;"
					>
						<!-- LUFS bars -->
						{#each [
							{ label: 'M', val: momentary, hold: holdM },
							{ label: 'S', val: shortterm, hold: holdS },
							{ label: 'I', val: integrated, hold: holdI }
						] as bar, i (bar.label)}
							<MeterBar
								class="flex-1 {i > 0 ? 'border-l border-neutral-300' : ''}"
								orientation="vertical"
								gradient={LUFS_GRADIENT}
								ghost
								hover
								hoverLabel={hoverLabel(LUFS_FLOOR)}
								pct={dbToPct(bar.val, LUFS_FLOOR)}
								hold={bar.hold > LUFS_SILENT ? dbToPct(bar.hold, LUFS_FLOOR) : null}
							/>
						{/each}

						{#if data.target != null}
							<div
								class="pointer-events-none absolute right-0 left-0 h-px bg-neutral-900"
								style="bottom: {dbToPct(data.target, LUFS_FLOOR)}%;"
							>
								<div class="absolute top-1/2 -right-px h-1.5 w-1.5 -translate-y-1/2 translate-x-full rotate-45 bg-neutral-900"></div>
							</div>
						{/if}
					</button>

					<!-- LUFS scale -->
					<div class="relative h-40 w-7 text-[8px] text-neutral-700 select-none">
						{#each LUFS_TICKS as db (db)}
							<div class="absolute left-0 flex items-center" style="bottom: {dbToPct(db, LUFS_FLOOR)}%; height: 1px;">
								<div class="h-px w-1.5 shrink-0 bg-neutral-400"></div>
								<span class="ml-0.5 leading-none">{db}</span>
							</div>
						{/each}
					</div>
				</div>

				<!-- M/S/I readout -->
				<div class="flex overflow-hidden rounded-sm border border-neutral-300 bg-neutral-100" style="width: {BAR_W * 3}px;">
					<div class="flex flex-1 flex-col items-center py-0.5">
						<span class="text-[7px] leading-none text-neutral-500">M</span>
						<span class="tabular-nums text-[8px] leading-tight">{format(momentary)}</span>
					</div>
					<div class="flex flex-1 flex-col items-center border-l border-neutral-300 py-0.5">
						<span class="text-[7px] leading-none text-neutral-500">S</span>
						<span class="tabular-nums text-[8px] leading-tight">{format(shortterm)}</span>
					</div>
					<div class="flex flex-1 flex-col items-center border-l border-neutral-300 py-0.5">
						<span class="text-[7px] leading-none text-neutral-500">I</span>
						<span class="tabular-nums text-[9px] leading-tight font-semibold {targetClass(integrated, data.target)}">{format(integrated)}</span>
					</div>
				</div>

				<div class="h-3 text-center text-[8px] leading-3 font-semibold {targetClass(integrated, data.target)}" style="width: {BAR_W * 3}px;">
					{targetDelta(integrated, data.target) ?? ''}
				</div>
			</div>
		</div>

		<!-- target presets -->
		<SegmentedButtons options={PRESETS} value={data.target ?? null} onSelect={setTarget} />
	</div>
</Wrapper>