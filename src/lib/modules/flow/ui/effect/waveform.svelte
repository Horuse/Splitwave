<script lang="ts">
	import { getContext } from 'svelte';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, NodeResizer, type Node, type NodeProps } from '@xyflow/svelte';
	import type { WaveformNodeData } from '$lib/modules/pipeline/types';
	import { Add, Minus, Pulse } from '$lib/components/icons';
	import { PREVIEW_CTX, channelColor, channelLabel } from '$lib/modules/flow/utils';
	import ChannelHandles from '../_channel_handles.svelte';

	const isPreview = getContext(PREVIEW_CTX) === true;

	type WaveformNodeType = Node<WaveformNodeData, 'waveform'>;
	let { id, data }: NodeProps<WaveformNodeType> = $props();

	const flow = useSvelteFlow();

	const FRAMES     = 1024;
	const MIN_SEGS   = 1;
	const MAX_SEGS   = 16;
	const MAX_BLOCKS = 300;
	const SCALE_W    = 30;
	const VERT_PAD   = 10;

	const SCALE_LEVELS: [number, string][] = [
		[ 1.0, '1.0'],
		[ 0.5, '0.5'],
		[ 0.0, '0.0'],
		[-0.5, '-0.5'],
		[-1.0, '-1.0'],
	];

	let segs = $state(data.segs ?? 4);
	let W    = $state(240);
	let Hpx  = $state(105);

	let channels = $state(1);
	let WW    = $derived(Math.max(1, W - SCALE_W));
	// viewBox height tracks the measured container exactly so the drawing fills
	// the node with no leftover strip; lanes divide it evenly.
	let H     = $derived(Hpx);
	let laneH = $derived(Hpx / channels);
	let halfH = $derived(Math.max(4, laneH / 2 - Math.min(VERT_PAD, laneH * 0.25)));

	let waveWrap: HTMLDivElement;

	$effect(() => {
		if (!waveWrap) return;
		const ro = new ResizeObserver((entries) => {
			const rect = entries[0].contentRect;
			const w = Math.round(rect.width);
			const h = Math.round(rect.height);
			requestAnimationFrame(() => {
				if (w > 0 && w !== W) W = w;
				if (h > 0 && h !== Hpx) { Hpx = h; dirty = true; }
			});
		});
		ro.observe(waveWrap);
		return () => ro.disconnect();
	});

	// Per-channel min/max envelope over WW columns, scrolled newest-on-right.
	// Filled incrementally per block so the full width fills continuously; the
	// block ring only backs a full rebuild on resize / zoom change.
	let peaks: Float32Array[] = [];
	let troughs: Float32Array[] = [];
	const blocks: number[][][] = [];
	let blockHead = 0;
	let blockCount = 0;
	let dirty = false;

	interface ScopeTick { nodeId: string; channels: number; data: number[][]; }

	function ensureArrays() {
		if (peaks.length === channels && peaks[0]?.length === WW) return;
		peaks = Array.from({ length: channels }, () => new Float32Array(WW));
		troughs = Array.from({ length: channels }, () => new Float32Array(WW));
	}

	function segEnvelope(buf: number[], seg: number, segSize: number): [number, number] {
		const i0 = seg * segSize;
		const i1 = Math.min(i0 + segSize, FRAMES);
		let p = 0, t = 0;
		for (let i = i0; i < i1; i++) {
			if (buf[i] > p) p = buf[i];
			if (buf[i] < t) t = buf[i];
		}
		return [Math.min(p, 1), Math.max(t, -1)];
	}

	function rebuildColumns() {
		ensureArrays();
		for (let c = 0; c < channels; c++) {
			peaks[c].fill(0);
			troughs[c].fill(0);
		}
		if (blockCount === 0) return;
		const segSize = Math.floor(FRAMES / segs);
		let col = WW - 1;
		for (let b = 0; b < blockCount && col >= 0; b++) {
			const bi = (blockHead + blockCount - 1 - b + MAX_BLOCKS) % MAX_BLOCKS;
			const blk = blocks[bi];
			for (let seg = segs - 1; seg >= 0 && col >= 0; seg--) {
				for (let c = 0; c < channels; c++) {
					const [p, t] = segEnvelope(blk[c], seg, segSize);
					peaks[c][col] = p;
					troughs[c][col] = t;
				}
				col--;
			}
		}
	}

	function pushBlock(block: number[][]) {
		if (block.length !== channels) {
			channels = block.length;
			blockCount = 0;
			blockHead = 0;
			ensureArrays();
		}
		ensureArrays();
		const idx = (blockHead + blockCount) % MAX_BLOCKS;
		if (blockCount < MAX_BLOCKS) blockCount++;
		else blockHead = (blockHead + 1) % MAX_BLOCKS;
		blocks[idx] = block;

		const segSize = Math.floor(FRAMES / segs);
		for (let c = 0; c < channels; c++) {
			peaks[c].copyWithin(0, segs);
			troughs[c].copyWithin(0, segs);
		}
		for (let s = 0; s < segs; s++) {
			const col = WW - segs + s;
			if (col < 0) continue;
			for (let c = 0; c < channels; c++) {
				const [p, t] = segEnvelope(block[c], s, segSize);
				peaks[c][col] = p;
				troughs[c][col] = t;
			}
		}
		dirty = true;
	}

	function changeSegs(delta: number) {
		segs = Math.min(MAX_SEGS, Math.max(MIN_SEGS, segs + delta));
		flow.updateNodeData(id, { segs });
		rebuildColumns();
		dirty = true;
	}

	// Width change resizes the envelope buffers -- refill from the block ring.
	let prevWW = 0;
	$effect(() => {
		const ww = WW;
		if (ww === prevWW) return;
		prevWW = ww;
		rebuildColumns();
		dirty = true;
	});

	let paths = $state<string[]>([]);

	function buildPaths() {
		const ww = WW;
		const out: string[] = new Array(channels);
		for (let c = 0; c < channels; c++) {
			const peak = peaks[c];
			const trough = troughs[c];
			if (!peak) {
				out[c] = '';
				continue;
			}
			let d = `M0,${(-peak[0] * halfH).toFixed(1)}`;
			for (let x = 1; x < ww; x++) d += ` L${x},${(-peak[x] * halfH).toFixed(1)}`;
			for (let x = ww - 1; x >= 0; x--) d += ` L${x},${(-trough[x] * halfH).toFixed(1)}`;
			out[c] = d + 'Z';
		}
		paths = out;
	}

	function updateFrame() {
		if (dirty) {
			dirty = false;
			buildPaths();
		}
		rafId = requestAnimationFrame(updateFrame);
	}

	let unlisten: UnlistenFn | undefined;
	let rafId: number | undefined;

	onMount(async () => {
		unlisten = await listen<ScopeTick>('audio://scope', (event) => {
			const p = event.payload;
			if (p.nodeId !== id) return;
			pushBlock(p.data);
		});
		rafId = requestAnimationFrame(updateFrame);
	});

	onDestroy(() => {
		unlisten?.();
		if (rafId !== undefined) cancelAnimationFrame(rafId);
	});
</script>

<div
	class={[
		'flex flex-col rounded-2xl border border-neutral-400 bg-neutral-200 shadow-sm',
		isPreview ? 'w-80 h-40' : 'w-full h-full'
	]}
>
	{#if !isPreview}
		<NodeResizer minWidth={160} maxWidth={1200} minHeight={80} maxHeight={1200} />
	{/if}

	<div class="flex shrink-0 items-center justify-between px-3 pt-2 pb-1">
		<span class="flex items-center gap-1.5 text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">
			<Pulse class="size-3 shrink-0 text-violet-600 dark:text-violet-400" />
			Waveform
		</span>
		<div class="flex items-center gap-1.5">
			<button
				type="button"
				class="nodrag nopan button-main primary size-4 p-0!"
				onclick={() => changeSegs(-1)}
				title="Zoom in"
			>
				<Minus class="size-2"/>
			</button>
			<span class="font-mono tabular-nums text-sm text-neutral-800 w-4 text-center">{segs}</span>
			<button
				type="button"
				class="nodrag nopan button-main primary size-4 p-0!"
				onclick={() => changeSegs(+1)}
				title="Zoom out"
			>
				<Add class="size-2"/>
			</button>
		</div>
	</div>

	<div class="flex min-h-0 flex-1 items-start px-4 pb-2">
		{#if !isPreview}
			<ChannelHandles nodeId={id} side="target" />
		{/if}
		<div bind:this={waveWrap} class="nowheel min-w-0 flex-1 self-stretch overflow-hidden">
		<!--
			viewBox ties the coordinate system to W×H (ResizeObserver-tracked).
			SVG itself renders at native device pixel density — no DPR math needed.
		-->
		<svg
			viewBox={`0 0 ${W} ${H}`}
			style="display:block; width:100%; height:100%;"
			aria-hidden="true"
		>
			<rect width={W} height={H} fill="#111" rx="10" />

			{#each paths as d, c (c)}
				{@const top = c * laneH}
				{@const color = channelColor(c)}
				<g transform={`translate(${SCALE_W},${top + laneH / 2})`}>
					<line x1="0" y1="0" x2={WW} y2="0"
					      stroke="rgba(255,255,255,0.12)" stroke-width="1"
					      shape-rendering="crispEdges" />
					{#if d}
						<path {d} fill={color} fill-opacity="0.7" stroke={color} stroke-width="0.75" stroke-linejoin="round" />
					{/if}
				</g>

				{#each SCALE_LEVELS as [amp, label]}
					{@const sy = top + laneH / 2 - amp * halfH}
					<rect x={SCALE_W - 3} y={sy - 0.5} width="3" height="1" fill="rgba(255,255,255,0.2)" />
					<text
						x={SCALE_W - 5} y={sy}
						fill={amp === 0 ? 'rgba(255,255,255,0.75)' : 'rgba(255,255,255,0.45)'}
						font-size="7.5" font-family="monospace"
						text-anchor="end" dominant-baseline="middle"
					>{label}</text>
				{/each}
				<line x1={SCALE_W - 1} y1={top} x2={SCALE_W - 1} y2={top + laneH}
				      stroke="rgba(255,255,255,0.12)" stroke-width="1" shape-rendering="crispEdges" />

				<!-- Channel tag, top of the lane just right of the scale rail. -->
				<text
					x={SCALE_W + 4} y={top + 9}
					fill={color} font-size="8" font-weight="bold" font-family="monospace"
					dominant-baseline="middle"
				>{channelLabel(c, channels)}</text>

				{#if c < channels - 1}
					<rect x="0" y={top + laneH} width={W} height="1" fill="rgba(255,255,255,0.08)" />
				{/if}
			{/each}
		</svg>
		</div>
		{#if !isPreview}
			<ChannelHandles nodeId={id} side="source" />
		{/if}
	</div>
</div>
