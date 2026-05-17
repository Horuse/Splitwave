<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, Handle, NodeResizer, Position, type Node, type NodeProps } from '@xyflow/svelte';
	import type { WaveformNodeData } from '$lib/modules/pipeline/types';
	import { Add, Minus } from '$lib/components/icons';

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
	let CH   = $state(52);
	let H    = $derived(CH * 2 + 1);
	let WW   = $derived(Math.max(1, W - SCALE_W));

	// Derived layout values used in template
	let halfH = $derived(CH / 2 - VERT_PAD);
	let rOff  = $derived(CH + 1); // top of channel R

	let waveWrap: HTMLDivElement;

	$effect(() => {
		if (!waveWrap) return;
		const ro = new ResizeObserver((entries) => {
			const rect = entries[0].contentRect;
			const w = Math.round(rect.width);
			const h = Math.round(rect.height);
			requestAnimationFrame(() => {
				if (w > 0 && w !== W) W = w;
				const newCH = Math.max(20, Math.floor(h / 2));
				if (newCH !== CH) { CH = newCH; dirty = true; }
			});
		});
		ro.observe(waveWrap);
		return () => ro.disconnect();
	});

	let peakL   = new Float32Array(WW);
	let troughL = new Float32Array(WW);
	let peakR   = new Float32Array(WW);
	let troughR = new Float32Array(WW);
	let prevWW  = WW;

	$effect(() => {
		if (WW === prevWW) return;
		prevWW = WW;
		peakL   = new Float32Array(WW);
		troughL = new Float32Array(WW);
		peakR   = new Float32Array(WW);
		troughR = new Float32Array(WW);
		rebuildColumns();
		dirty = true;
	});

	const blockL: Float32Array[] = Array.from({ length: MAX_BLOCKS }, () => new Float32Array(FRAMES));
	const blockR: Float32Array[] = Array.from({ length: MAX_BLOCKS }, () => new Float32Array(FRAMES));
	let blockHead  = 0;
	let blockCount = 0;

	let hasData = false;
	let dirty   = false;

	function rebuildColumns() {
		const ww = peakL.length;
		peakL.fill(0); troughL.fill(0);
		peakR.fill(0); troughR.fill(0);
		if (blockCount === 0) return;
		const segSize = Math.floor(FRAMES / segs);
		let col = 0;
		for (let b = 0; b < blockCount && col < ww; b++) {
			const bi = (blockHead + blockCount - 1 - b + MAX_BLOCKS) % MAX_BLOCKS;
			const lBuf = blockL[bi];
			const rBuf = blockR[bi];
			for (let seg = segs - 1; seg >= 0 && col < ww; seg--) {
				const i0 = seg * segSize;
				const i1 = Math.min(i0 + segSize, FRAMES);
				let pl = 0, tl = 0, pr = 0, tr = 0;
				for (let i = i0; i < i1; i++) {
					if (lBuf[i] > pl) pl = lBuf[i];
					if (lBuf[i] < tl) tl = lBuf[i];
					if (rBuf[i] > pr) pr = rBuf[i];
					if (rBuf[i] < tr) tr = rBuf[i];
				}
				peakL[col]   = Math.min(pl,  1);
				troughL[col] = Math.max(tl, -1);
				peakR[col]   = Math.min(pr,  1);
				troughR[col] = Math.max(tr, -1);
				col++;
			}
		}
	}

	interface ScopeTick { nodeId: string; l: number[]; r: number[]; }

	function changeSegs(delta: number) {
		segs = Math.min(MAX_SEGS, Math.max(MIN_SEGS, segs + delta));
		flow.updateNodeData(id, { segs });
		rebuildColumns();
		dirty = true;
	}

	function pushBlock(l: number[], r: number[]) {
		const writeIdx = (blockHead + blockCount) % MAX_BLOCKS;
		if (blockCount < MAX_BLOCKS) blockCount++;
		else blockHead = (blockHead + 1) % MAX_BLOCKS;
		blockL[writeIdx].set(l);
		blockR[writeIdx].set(r);

		peakL.copyWithin(segs, 0);
		troughL.copyWithin(segs, 0);
		peakR.copyWithin(segs, 0);
		troughR.copyWithin(segs, 0);

		const segSize = Math.floor(FRAMES / segs);
		for (let col = 0; col < segs; col++) {
			const seg = segs - 1 - col;
			const i0 = seg * segSize;
			const i1 = Math.min(i0 + segSize, FRAMES);
			let pl = 0, tl = 0, pr = 0, tr = 0;
			for (let i = i0; i < i1; i++) {
				if (l[i] > pl) pl = l[i];
				if (l[i] < tl) tl = l[i];
				if (r[i] > pr) pr = r[i];
				if (r[i] < tr) tr = r[i];
			}
			peakL[col]   = Math.min(pl,  1);
			troughL[col] = Math.max(tl, -1);
			peakR[col]   = Math.min(pr,  1);
			troughR[col] = Math.max(tr, -1);
		}

		hasData = true;
		dirty = true;
	}

	let svgPathL = $state('');
	let svgPathR = $state('');

	function buildPath(peak: Float32Array, trough: Float32Array, h: number): string {
		const n = peak.length;
		if (n === 0) return '';
		let d = `M0,${(-peak[0] * h).toFixed(1)}`;
		for (let x = 1; x < n; x++) d += ` L${x},${(-peak[x] * h).toFixed(1)}`;
		for (let x = n - 1; x >= 0; x--) d += ` L${x},${(-trough[x] * h).toFixed(1)}`;
		return d + 'Z';
	}

	function updateFrame() {
		if (dirty && hasData) {
			dirty = false;
			svgPathL = buildPath(peakL, troughL, halfH);
			svgPathR = buildPath(peakR, troughR, halfH);
		}
		rafId = requestAnimationFrame(updateFrame);
	}

	let unlisten: UnlistenFn | undefined;
	let rafId: number | undefined;

	onMount(async () => {
		unlisten = await listen<ScopeTick>('audio://scope', (event) => {
			const p = event.payload;
			if (p.nodeId !== id) return;
			pushBlock(p.l, p.r);
		});
		rafId = requestAnimationFrame(updateFrame);
	});

	onDestroy(() => {
		unlisten?.();
		if (rafId !== undefined) cancelAnimationFrame(rafId);
	});
</script>

<div class="w-full h-full flex flex-col rounded-2xl border border-neutral-400 bg-neutral-200 shadow-sm">
	<NodeResizer minWidth={160} maxWidth={1200} minHeight={80} maxHeight={1200}  />

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

			<!-- ── Channel L ─────────────────────────────────────── -->
			<g transform={`translate(${SCALE_W},${CH / 2})`}>
				<line x1="0" y1="0" x2={WW} y2="0"
				      stroke="rgba(255,255,255,0.12)" stroke-width="1"
				      shape-rendering="crispEdges" />
				{#if svgPathL}
					<path d={svgPathL} fill="#22c55e" fill-opacity="0.75" stroke="#4ade80" stroke-width="0.75" stroke-linejoin="round" />

				{/if}
			</g>

			<!-- Scale L -->
			{#each SCALE_LEVELS as [amp, label]}
				{@const sy = CH / 2 - amp * halfH}
				<rect x={SCALE_W - 3} y={sy - 0.5} width="3" height="1"
				      fill="rgba(255,255,255,0.2)" />
				<text
					x={SCALE_W - 5} y={sy}
					fill={amp === 0 ? 'rgba(255,255,255,0.75)' : 'rgba(255,255,255,0.45)'}
					font-size="7.5" font-family="monospace"
					text-anchor="end" dominant-baseline="middle"
				>{label}</text>
			{/each}
			<line x1={SCALE_W - 1} y1="0" x2={SCALE_W - 1} y2={CH}
			      stroke="rgba(255,255,255,0.12)" stroke-width="1" shape-rendering="crispEdges" />

			<!-- Channel separator -->
			<rect x="0" y={CH} width={W} height="1" fill="rgba(255,255,255,0.08)" />

			<!-- ── Channel R ─────────────────────────────────────── -->
			<g transform={`translate(${SCALE_W},${rOff + CH / 2})`}>
				<line x1="0" y1="0" x2={WW} y2="0"
				      stroke="rgba(255,255,255,0.12)" stroke-width="1"
				      shape-rendering="crispEdges" />
				{#if svgPathR}
					<path d={svgPathR} fill="#22c55e" fill-opacity="0.75" stroke="#4ade80" stroke-width="0.75" stroke-linejoin="round" />
				{/if}
			</g>

			<!-- Scale R -->
			{#each SCALE_LEVELS as [amp, label]}
				{@const sy = rOff + CH / 2 - amp * halfH}
				<rect x={SCALE_W - 3} y={sy - 0.5} width="3" height="1"
				      fill="rgba(255,255,255,0.2)" />
				<text
					x={SCALE_W - 5} y={sy}
					fill={amp === 0 ? 'rgba(255,255,255,0.75)' : 'rgba(255,255,255,0.45)'}
					font-size="7.5" font-family="monospace"
					text-anchor="end" dominant-baseline="middle"
				>{label}</text>
			{/each}
			<line x1={SCALE_W - 1} y1={rOff} x2={SCALE_W - 1} y2={H}
			      stroke="rgba(255,255,255,0.12)" stroke-width="1" shape-rendering="crispEdges" />
		</svg>
	</div>

	<Handle type="target" position={Position.Left} class="handle" />
	<Handle type="source" position={Position.Right} class="handle" />
</div>