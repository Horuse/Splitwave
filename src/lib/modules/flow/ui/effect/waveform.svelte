<script lang="ts">
	import { getContext } from 'svelte';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, Handle, NodeResizer, Position, type Node, type NodeProps } from '@xyflow/svelte';
	import type { WaveformNodeData } from '$lib/modules/pipeline/types';
	import { Add, Minus } from '$lib/components/icons';
	import { PREVIEW_CTX, channelColor } from '$lib/modules/flow/utils';

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
	let WW   = $derived(Math.max(1, W - SCALE_W));
	let laneH = $derived(Math.max(20, Math.floor((Hpx - (channels - 1)) / channels)));
	let H     = $derived(laneH * channels + (channels - 1));
	let halfH = $derived(Math.max(4, laneH / 2 - Math.min(VERT_PAD, laneH * 0.2)));

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

	// Ring of recent blocks; each block is channels x FRAMES.
	const blocks: number[][][] = [];
	let dirty = false;

	interface ScopeTick { nodeId: string; channels: number; data: number[][]; }

	function pushBlock(block: number[][]) {
		if (block.length !== channels) {
			channels = block.length;
			blocks.length = 0;
		}
		blocks.push(block);
		if (blocks.length > MAX_BLOCKS) blocks.shift();
		dirty = true;
	}

	function changeSegs(delta: number) {
		segs = Math.min(MAX_SEGS, Math.max(MIN_SEGS, segs + delta));
		flow.updateNodeData(id, { segs });
		dirty = true;
	}

	let paths = $state<string[]>([]);

	function buildPaths() {
		const ww = WW;
		const segSize = Math.floor(FRAMES / segs);
		const out: string[] = new Array(channels);
		for (let c = 0; c < channels; c++) {
			const peak = new Float32Array(ww);
			const trough = new Float32Array(ww);
			let col = ww - 1;
			for (let b = blocks.length - 1; b >= 0 && col >= 0; b--) {
				const buf = blocks[b][c];
				if (!buf) continue;
				for (let seg = segs - 1; seg >= 0 && col >= 0; seg--) {
					const i0 = seg * segSize;
					const i1 = Math.min(i0 + segSize, FRAMES);
					let p = 0, t = 0;
					for (let i = i0; i < i1; i++) {
						if (buf[i] > p) p = buf[i];
						if (buf[i] < t) t = buf[i];
					}
					peak[col] = Math.min(p, 1);
					trough[col] = Math.max(t, -1);
					col--;
				}
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
		<span class="text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">Waveform</span>
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

	<div bind:this={waveWrap} class="nowheel min-h-0 flex-1 px-2 pb-2 overflow-hidden">
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
				{@const top = c * (laneH + 1)}
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

				{#if c < channels - 1}
					<rect x="0" y={top + laneH} width={W} height="1" fill="rgba(255,255,255,0.08)" />
				{/if}
			{/each}
		</svg>
	</div>

	{#if !isPreview}
		<Handle type="target" position={Position.Left} class="handle" />
		<Handle type="source" position={Position.Right} class="handle" />
	{/if}
</div>
