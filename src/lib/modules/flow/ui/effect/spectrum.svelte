<script lang="ts">
	import { getContext, onDestroy, onMount } from 'svelte';
	import { tauriListen } from '$lib/utils/tauri_event';
	import { useSvelteFlow, NodeResizer, type Node, type NodeProps } from '@xyflow/svelte';
	import type { SpectrumNodeData } from '$lib/modules/pipeline/types';
	import { DataBar } from '$lib/components/icons';
	import { PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { CATEGORY_TEXT } from '$lib/modules/flow/utils/accents';
	import ChannelHandles from '../_channel_handles.svelte';
	import { formatFreq } from '$lib/components/format';

	const isPreview = getContext(PREVIEW_CTX) === true;

	type SpectrumNodeType = Node<SpectrumNodeData, 'spectrum'>;
	let { id, data }: NodeProps<SpectrumNodeType> = $props();

	const flow = useSvelteFlow();

	let smoothing = $state(data.smoothing ?? 0.5);

	function setSmoothing(v: number) {
		smoothing = v;
		flow.updateNodeData(id, { smoothing: v });
	}

	const BARS = 80;
	const F_MIN = 20;
	const DB_FLOOR = -96;

	let sampleRate = $state(48_000);
	let fMax = $derived(sampleRate / 2);

	const SCALE_W = 28;
	const RIGHT_PAD = 12;
	const TOP_PAD = 9;
	const AXIS_H = 12;

	const DB_TICKS = [0, -12, -24, -36, -48, -60, -72, -84, -96];

	let W = $state(260);
	let Hpx = $state(140);
	let plotWrap: HTMLDivElement;
	let canvas: HTMLCanvasElement;
	let ctx: CanvasRenderingContext2D | null = null;

	$effect(() => {
		if (!plotWrap) return;
		const ro = new ResizeObserver((entries) => {
			const rect = entries[0].contentRect;
			const w = Math.round(rect.width);
			const h = Math.round(rect.height);
			requestAnimationFrame(() => {
				if (w > 0 && w !== W) W = w;
				if (h > 0 && h !== Hpx) Hpx = h;
			});
		});
		ro.observe(plotWrap);
		return () => ro.disconnect();
	});

	let plotW = $derived(Math.max(1, W - SCALE_W - RIGHT_PAD));
	let plotH = $derived(Math.max(1, Hpx - AXIS_H));

	let freqTicks = $derived.by<{ f: number; label: string; major: boolean }[]>(() => {
		const out: { f: number; label: string; major: boolean }[] = [];
		for (let d = 10; d <= fMax; d *= 10) {
			for (const m of [1, 2, 5]) {
				const f = d * m;
				if (f < F_MIN || f > fMax) continue;
				out.push({ f, label: formatFreq(f), major: m === 1 });
			}
		}
		return out;
	});

	const barDb = new Float32Array(BARS).fill(DB_FLOOR);
	let hasSignal = false;

	function dbToY(db: number): number {
		const t = (db - DB_FLOOR) / -DB_FLOOR; // 0 at floor, 1 at 0 dB
		return plotH - Math.max(0, Math.min(1, t)) * (plotH - TOP_PAD);
	}

	function freqX(f: number): number {
		const t = Math.log(f / F_MIN) / Math.log(fMax / F_MIN);
		return SCALE_W + plotW * Math.max(0, Math.min(1, t));
	}

	function updateBars(bars: number[], sr: number) {
		if (sr > 0) sampleRate = sr;
		const rise = 1 - 0.85 * smoothing;
		const fall = 0.4 - 0.37 * smoothing;
		for (let b = 0; b < BARS; b++) {
			const target = bars[b] ?? DB_FLOOR;
			const cur = barDb[b];
			barDb[b] = cur + (target - cur) * (target > cur ? rise : fall);
		}
		hasSignal = true;
	}

	function renderCanvas() {
		if (!canvas) return;
		if (!ctx) ctx = canvas.getContext('2d');
		if (!ctx) return;

		const dpr = window.devicePixelRatio || 1;
		const bw = Math.max(1, Math.round(W * dpr));
		const bh = Math.max(1, Math.round(Hpx * dpr));
		if (canvas.width !== bw) canvas.width = bw;
		if (canvas.height !== bh) canvas.height = bh;
		if (canvas.style.width !== `${W}px`) canvas.style.width = `${W}px`;
		if (canvas.style.height !== `${Hpx}px`) canvas.style.height = `${Hpx}px`;

		ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		ctx.clearRect(0, 0, W, Hpx);
		if (!hasSignal) return;

		const step = plotW / BARS;
		for (let b = 0; b < BARS; b++) {
			const y = dbToY(barDb[b]);
			const h = plotH - y;
			if (h > 0) {
				ctx.fillStyle = `hsl(${210 - (b / BARS) * 210} 85% 55%)`;
				ctx.fillRect(SCALE_W + b * step, y, Math.max(1, step - 1), h);
			}
		}
	}

	let rafId: number | undefined;
	function frame() {
		if (hasSignal) renderCanvas();
		rafId = requestAnimationFrame(frame);
	}

	interface SpectrumTick {
		nodeId: string;
		bars: number[];
		sampleRate: number;
	}

	let unlisten: (() => void) | undefined;

	onMount(() => {
		unlisten = tauriListen<SpectrumTick>('audio://spectrum', (p) => {
			if (p.nodeId !== id) return;
			updateBars(p.bars, p.sampleRate);
		});
		rafId = requestAnimationFrame(frame);
	});

	onDestroy(() => {
		unlisten?.();
		if (rafId !== undefined) cancelAnimationFrame(rafId);
	});
</script>

<div class={['flex flex-col rounded-2xl border border-neutral-400 bg-neutral-200 shadow-sm', isPreview ? 'h-40 w-80' : 'h-full w-full']}>
	{#if !isPreview}
		<NodeResizer minWidth={200} maxWidth={1400} minHeight={110} maxHeight={1400} />
	{/if}

	<div class="flex shrink-0 items-center justify-between gap-2 px-3 pt-2 pb-1">
		<span class="flex items-center gap-1.5 text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">
			<DataBar class={['size-3 shrink-0', CATEGORY_TEXT.monitor]} />
			Spectrum
		</span>
		{#if !isPreview}
			<label class="flex items-center gap-1.5 text-[9px] text-neutral-600">
				<span class="shrink-0">Smooth</span>
				<input
					type="range"
					class="nodrag nopan nowheel w-[100px] accent-amber-500"
					min="0"
					max="1"
					step="0.01"
					value={smoothing}
					oninput={(e) => setSmoothing(e.currentTarget.valueAsNumber)}
					title={`Smoothing ${Math.round(smoothing * 100)}%`} />
				<span class="flex items-baseline">
					<input
						type="number"
						min="0"
						max="100"
						step="1"
						class="no-spin nodrag nopan w-7 rounded bg-transparent text-right font-mono text-[9px] tabular-nums focus:bg-neutral-100 focus:ring-1 focus:ring-amber-500 focus:outline-none"
						value={Math.round(smoothing * 100)}
						oninput={(e) => {
							const n = e.currentTarget.valueAsNumber;
							if (!Number.isNaN(n)) setSmoothing(Math.max(0, Math.min(1, n / 100)));
						}} />
					<span class="text-neutral-400">%</span>
				</span>
			</label>
		{/if}
	</div>

	<div class="flex min-h-0 flex-1 items-start px-4 pb-2">
		{#if !isPreview}
			<ChannelHandles nodeId={id} side="target" />
		{/if}
		<div bind:this={plotWrap} class="nowheel relative min-w-0 flex-1 self-stretch overflow-hidden rounded-[10px]">
			<!-- Flat canvas for high-perf 60 FPS spectrum bars without DOM nodes -->
			<canvas
				bind:this={canvas}
				class="pointer-events-none absolute inset-0 block h-full w-full bg-[#111]"
				aria-hidden="true"></canvas>

			<!-- Crisp vector overlay for scales, axes and grid -->
			<svg viewBox={`0 0 ${W} ${Hpx}`} style="display:block; width:100%; height:100%;" class="pointer-events-none relative z-10" aria-hidden="true">
				<!-- dB grid + scale rail -->
				{#each DB_TICKS as db (db)}
					{@const y = dbToY(db)}
					<line x1={SCALE_W} y1={y} x2={W - RIGHT_PAD} y2={y} stroke="rgba(255,255,255,0.07)" stroke-width="1" shape-rendering="crispEdges" />
					<text x={SCALE_W - 4} {y} fill="rgba(255,255,255,0.45)" font-size="7" font-family="monospace" text-anchor="end" dominant-baseline="middle"
						>{db}</text>
				{/each}

				<!-- frequency grid -->
				{#each freqTicks as t (t.f)}
					{@const x = freqX(t.f)}
					<line
						x1={x}
						y1={TOP_PAD}
						x2={x}
						y2={plotH}
						stroke={t.major ? 'rgba(255,255,255,0.12)' : 'rgba(255,255,255,0.05)'}
						stroke-width="1"
						shape-rendering="crispEdges" />
					<text
						{x}
						y={plotH + 8}
						fill={t.major ? 'rgba(255,255,255,0.6)' : 'rgba(255,255,255,0.38)'}
						font-size="6.5"
						font-family="monospace"
						text-anchor="middle"
						dominant-baseline="middle">{t.label}</text>
				{/each}

				<line
					x1={W - RIGHT_PAD}
					y1={TOP_PAD}
					x2={W - RIGHT_PAD}
					y2={plotH}
					stroke="rgba(255,255,255,0.12)"
					stroke-width="1"
					shape-rendering="crispEdges" />
			</svg>
		</div>
		{#if !isPreview}
			<ChannelHandles nodeId={id} side="source" />
		{/if}
	</div>
</div>

<style>
	.no-spin::-webkit-outer-spin-button,
	.no-spin::-webkit-inner-spin-button {
		appearance: none;
		margin: 0;
	}
	.no-spin {
		appearance: textfield;
	}
</style>
