<script lang="ts">
	import { getContext, onDestroy, onMount } from 'svelte';
	import { tauriListen } from '$lib/utils/tauri_event';
	import { useSvelteFlow, NodeResizer, type Node, type NodeProps } from '@xyflow/svelte';
	import type { SpectrumNodeData } from '$lib/modules/pipeline/types';
	import { DataBar } from '$lib/components/icons';
	import { PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { CATEGORY_TEXT } from '$lib/modules/flow/utils/accents';
	import ChannelHandles from '../_channel_handles.svelte';

	const isPreview = getContext(PREVIEW_CTX) === true;

	type SpectrumNodeType = Node<SpectrumNodeData, 'spectrum'>;
	let { id, data }: NodeProps<SpectrumNodeType> = $props();

	const flow = useSvelteFlow();

	let smoothing = $state(data.smoothing ?? 0.5);

	function setSmoothing(v: number) {
		smoothing = v;
		flow.updateNodeData(id, { smoothing: v });
	}

	// The engine ships one contiguous 4096-frame window per spectrum node
	// (SPECTRUM_FRAMES) so the low end resolves (~11.7 Hz/bin at 48 kHz) without
	// the discontinuities that concatenating separate 1024 snapshots would cause.
	const FFT_SIZE = 4096;
	const BINS = FFT_SIZE / 2;
	const MAX_CH = 8; // per-channel FFTs beyond this cost more than they reveal
	const F_MIN = 20;
	const BARS = 80; // log-spaced across the audible range
	const DB_FLOOR = -96;

	// Monitors run at the max input rate, not always 48 kHz; the scope event
	// carries the real rate so the frequency axis stays honest.
	let sampleRate = $state(48_000);
	let fMax = $derived(sampleRate / 2);

	const SCALE_W = 28;
	const RIGHT_PAD = 12; // mirrors the gap the dB rail leaves on the left
	const TOP_PAD = 9; // headroom so the 0 dB line and its label aren't clipped
	const AXIS_H = 12;

	const DB_TICKS = [0, -12, -24, -36, -48, -60, -72, -84, -96];

	let W = $state(260);
	let Hpx = $state(140);
	let plotWrap: HTMLDivElement;

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

	// Bar band edges in bin space, log-spaced. Recomputed if the monitor rate
	// changes (a graph rebuild), else stable.
	let bandBins = $derived.by<[number, number][]>(() => {
		const out: [number, number][] = [];
		for (let b = 0; b < BARS; b++) {
			const f0 = F_MIN * Math.pow(fMax / F_MIN, b / BARS);
			const f1 = F_MIN * Math.pow(fMax / F_MIN, (b + 1) / BARS);
			const lo = Math.max(1, Math.floor((f0 / fMax) * BINS));
			const hi = Math.max(lo + 1, Math.ceil((f1 / fMax) * BINS));
			out.push([lo, Math.min(hi, BINS)]);
		}
		return out;
	});

	// Log-spaced 1-2-5 frequency ticks over the audible range.
	let freqTicks = $derived.by<{ f: number; label: string; major: boolean }[]>(() => {
		const out: { f: number; label: string; major: boolean }[] = [];
		for (let d = 10; d <= fMax; d *= 10) {
			for (const m of [1, 2, 5]) {
				const f = d * m;
				if (f < F_MIN || f > fMax) continue;
				out.push({ f, label: f >= 1000 ? `${f / 1000}k` : `${f}`, major: m === 1 });
			}
		}
		return out;
	});

	const hann = new Float32Array(FFT_SIZE);
	for (let i = 0; i < FFT_SIZE; i++) {
		hann[i] = 0.5 * (1 - Math.cos((2 * Math.PI * i) / (FFT_SIZE - 1)));
	}

	const re = new Float32Array(FFT_SIZE);
	const im = new Float32Array(FFT_SIZE);
	const bandPeak = new Float32Array(BARS);
	const barDb = new Float32Array(BARS).fill(DB_FLOOR);
	let hasSignal = false;

	// In-place iterative radix-2 FFT.
	function fft(reBuf: Float32Array, imBuf: Float32Array) {
		const n = reBuf.length;
		for (let i = 1, j = 0; i < n; i++) {
			let bit = n >> 1;
			for (; j & bit; bit >>= 1) j ^= bit;
			j ^= bit;
			if (i < j) {
				[reBuf[i], reBuf[j]] = [reBuf[j], reBuf[i]];
				[imBuf[i], imBuf[j]] = [imBuf[j], imBuf[i]];
			}
		}
		for (let len = 2; len <= n; len <<= 1) {
			const ang = (-2 * Math.PI) / len;
			const wr = Math.cos(ang);
			const wi = Math.sin(ang);
			const half = len >> 1;
			for (let i = 0; i < n; i += len) {
				let cr = 1;
				let ci = 0;
				for (let k = 0; k < half; k++) {
					const ar = reBuf[i + k];
					const ai = imBuf[i + k];
					const br = reBuf[i + k + half] * cr - imBuf[i + k + half] * ci;
					const bi = reBuf[i + k + half] * ci + imBuf[i + k + half] * cr;
					reBuf[i + k] = ar + br;
					imBuf[i + k] = ai + bi;
					reBuf[i + k + half] = ar - br;
					imBuf[i + k + half] = ai - bi;
					const ncr = cr * wr - ci * wi;
					ci = cr * wi + ci * wr;
					cr = ncr;
				}
			}
		}
	}

	function analyze(chans: number[][], sr: number) {
		const ch = chans.length;
		if (ch === 0) return;
		if (sr > 0) sampleRate = sr;
		const cap = Math.min(ch, MAX_CH);
		const n = Math.min(FFT_SIZE, chans[0].length);
		const bands = bandBins;
		bandPeak.fill(0);
		// Per-channel FFT combined by max: a summed mono downmix would cancel
		// out-of-phase content and hide real energy.
		for (let c = 0; c < cap; c++) {
			const src = chans[c];
			for (let i = 0; i < n; i++) {
				re[i] = (src[i] ?? 0) * hann[i];
				im[i] = 0;
			}
			for (let i = n; i < FFT_SIZE; i++) {
				re[i] = 0;
				im[i] = 0;
			}
			fft(re, im);
			for (let b = 0; b < BARS; b++) {
				const [lo, hi] = bands[b];
				let peak = bandPeak[b];
				for (let k = lo; k < hi; k++) {
					const mag = Math.hypot(re[k], im[k]);
					if (mag > peak) peak = mag;
				}
				bandPeak[b] = peak;
			}
		}
		// Higher smoothing → smaller step toward target, so bars glide instead of
		// jittering. Rise faster than fall so transients still read.
		const rise = 1 - 0.85 * smoothing;
		const fall = 0.4 - 0.37 * smoothing;
		for (let b = 0; b < BARS; b++) {
			const norm = bandPeak[b] / BINS;
			const target = norm > 1e-7 ? Math.max(DB_FLOOR, 20 * Math.log10(norm)) : DB_FLOOR;
			const cur = barDb[b];
			barDb[b] = cur + (target - cur) * (target > cur ? rise : fall);
		}
		hasSignal = true;
	}

	function dbToY(db: number): number {
		const t = (db - DB_FLOOR) / -DB_FLOOR; // 0 at floor, 1 at 0 dB
		return plotH - Math.max(0, Math.min(1, t)) * (plotH - TOP_PAD);
	}

	function freqX(f: number): number {
		const t = Math.log(f / F_MIN) / Math.log(fMax / F_MIN);
		return SCALE_W + plotW * Math.max(0, Math.min(1, t));
	}

	let bars = $state<{ x: number; w: number; y: number; h: number; hue: number }[]>([]);

	function buildFrame() {
		const step = plotW / BARS;
		const out = new Array(BARS);
		for (let b = 0; b < BARS; b++) {
			const y = dbToY(barDb[b]);
			out[b] = {
				x: SCALE_W + b * step,
				w: Math.max(1, step - 1),
				y,
				h: plotH - y,
				hue: 210 - (b / BARS) * 210
			};
		}
		bars = out;
	}

	let rafId: number | undefined;
	function frame() {
		if (hasSignal) buildFrame();
		rafId = requestAnimationFrame(frame);
	}

	interface ScopeTick {
		nodeId: string;
		channels: number;
		data: number[][];
		sampleRate: number;
	}

	tauriListen<ScopeTick>('audio://scope', (p) => {
		if (p.nodeId !== id) return;
		analyze(p.data, p.sampleRate);
	});

	onMount(() => {
		rafId = requestAnimationFrame(frame);
	});

	onDestroy(() => {
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
		<div bind:this={plotWrap} class="nowheel min-w-0 flex-1 self-stretch overflow-hidden">
			<svg viewBox={`0 0 ${W} ${Hpx}`} style="display:block; width:100%; height:100%;" aria-hidden="true">
				<rect width={W} height={Hpx} fill="#111" rx="10" />

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

				{#each bars as bar, b (b)}
					<rect x={bar.x} y={bar.y} width={bar.w} height={bar.h} fill={`hsl(${bar.hue} 85% 55%)`} rx="1" />
				{/each}
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
