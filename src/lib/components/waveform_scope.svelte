<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { channelColor, channelLabel } from '$lib/modules/flow/utils';
	import { methods } from '$lib/modules/audio/methods';

	// Scope-style waveform viewer shared by the Waveform node and the File
	// Recording node. Incoming scope blocks are pre-binned into fixed segments
	// (block-aligned, so binning is stable), then the view re-aggregates those
	// segments under a sub-pixel scroll offset. Because columns only re-bin when
	// the grid advances a whole column, the envelope never shimmers while new
	// audio streams in. Interactions mirror the flow editor: wheel zooms, drag
	// pans, shift+wheel pans, double-click resets to the live edge.
	//
	// Rendered on a single Canvas 2D surface scaled by devicePixelRatio for
	// retina. Envelope columns are drawn directly from precomputed Float32
	// min/max buffers, with no Svelte reactivity in the hot path. Repaints are
	// coalesced through rAF and only run when something actually changed.
	let {
		nodeId,
		height = 140,
		fill = false,
		pan = true,
		filePath = null
	}: { nodeId: string; height?: number; fill?: boolean; pan?: boolean; filePath?: string | null } = $props();

	const SEG_FRAMES = 64;
	const CAP_SEGS = (300 * 1024) / SEG_FRAMES;
	const DEFAULT_SEGS = 20; // fixed "×1" reference, so max zoom (1 seg/px) reads ×20 at any sample rate
	const TIME_H = 18;
	const SCALE_W = 30;
	const VERT_PAD = 10;
	const SCROLLBAR_HIT = 10;
	const SCALE_LEVELS: [number, string][] = [
		[1.0, '1.0'],
		[0.5, '0.5'],
		[0.0, '0.0'],
		[-0.5, '-0.5'],
		[-1.0, '-1.0']
	];

	interface ScopeTick {
		nodeId: string;
		channels: number;
		data: number[][];
		sampleRate?: number;
	}

	let channels = 1;
	let sampleRate = 48_000;
	let segsPerCol = DEFAULT_SEGS;
	let zoomF = DEFAULT_SEGS; // fractional zoom accumulator; source of truth for segsPerCol
	let defaultSegs = DEFAULT_SEGS;
	let viewEndSeg = 0;
	let following = true;
	let zoomInit = false;

	// Segment min/max rings (block-aligned, immutable once written).
	let minRing = new Float32Array(CAP_SEGS);
	let maxRing = new Float32Array(CAP_SEGS);
	let writeSeg = 0;
	let totalSegs = 0;

	// File mode (WAV/AIFF only): the whole recording is browsable by lazily
	// loading min/max bins from disk for the visible range instead of holding
	// every sample in RAM.
	let fileMode = $derived(isPcm(filePath));
	let fileCache = new Map<number, Float32Array>();
	let fileTotalSegs = 0;
	let fileChannels = 0;
	let fileLoaded = false;
	let fetching = false;
	let lastTailCheck = 0;

	function isPcm(p: string | null | undefined): boolean {
		if (!p) return false;
		const lower = p.toLowerCase();
		return lower.endsWith('.wav') || lower.endsWith('.aiff') || lower.endsWith('.aif');
	}

	function dataStart(): number {
		return fileMode ? 0 : Math.max(0, totalSegs - CAP_SEGS);
	}

	function dataCap(): number {
		return fileMode ? fileTotalSegs : CAP_SEGS;
	}

	let W = $state(0);
	let H = $state(height);

	// Draw-state produced by `rebuild` and consumed by `draw` (plain, non-reactive).
	let colsCount = 0;
	let peaks: Float32Array[] = [];
	let troughs: Float32Array[] = [];
	let off = 0;
	let ticks: { x: number; label: string }[] = [];

	let wrap: HTMLDivElement;
	let canvas: HTMLCanvasElement;
	let ctx: CanvasRenderingContext2D | null = null;
	let rafId = 0;
	let rafScheduled = false;
	let dirty = true;
	let dragging = $state(false);
	let scrollbarDragging = false;
	let zoomLabel = $state('×1.0');
	let lastX = 0;

	function ensureRing(ch: number) {
		if (ch === channels && minRing.length === CAP_SEGS * ch) return;
		channels = ch;
		minRing = new Float32Array(CAP_SEGS * ch);
		maxRing = new Float32Array(CAP_SEGS * ch);
		writeSeg = 0;
		totalSegs = 0;
		viewEndSeg = 0;
		following = true;
	}

	function segSlot(d: number): number {
		let slot = (writeSeg - 1 - d) % CAP_SEGS;
		if (slot < 0) slot += CAP_SEGS;
		return slot;
	}

	function segEnvelope(seg: number, c: number): [number, number] | null {
		if (fileMode) {
			const e = fileCache.get(seg);
			return e ? [e[c * 2], e[c * 2 + 1]] : null;
		}
		const base = segSlot(totalSegs - 1 - seg) * channels + c;
		return [minRing[base], maxRing[base]];
	}

	function onScope(p: ScopeTick) {
		if (p.nodeId !== nodeId) return;
		if (fileMode) {
			// Live signal that the recording file grew; refresh the tail (a
			// no-op fetch unless new data has actually been flushed to disk).
			if (following) markDirty();
			return;
		}
		ensureRing(p.channels);
		if (p.sampleRate) {
			if (!zoomInit) {
				sampleRate = p.sampleRate;
				zoomF = DEFAULT_SEGS;
				defaultSegs = DEFAULT_SEGS;
				segsPerCol = DEFAULT_SEGS;
				updateZoomLabel();
				zoomInit = true;
			} else {
				sampleRate = p.sampleRate;
			}
		}
		const ch = p.channels;
		const frames = p.data[0]?.length ?? 0;
		if (frames === 0) return;
		const segsInBlock = Math.max(1, Math.ceil(frames / SEG_FRAMES));
		for (let s = 0; s < segsInBlock; s++) {
			const f0 = s * SEG_FRAMES;
			const f1 = Math.min(f0 + SEG_FRAMES, frames);
			const slot = (writeSeg + s) % CAP_SEGS;
			const base = slot * ch;
			for (let c = 0; c < ch; c++) {
				let mn = Infinity;
				let mx = -Infinity;
				for (let f = f0; f < f1; f++) {
					const v = p.data[c][f];
					if (v < mn) mn = v;
					if (v > mx) mx = v;
				}
				if (f0 >= f1) {
					mn = 0;
					mx = 0;
				}
				minRing[base + c] = mn;
				maxRing[base + c] = mx;
			}
		}
		writeSeg = (writeSeg + segsInBlock) % CAP_SEGS;
		totalSegs += segsInBlock;
		if (following) viewEndSeg = totalSegs;
		markDirty();
	}

	function clampSegs() {
		const plotW = Math.max(1, W - SCALE_W);
		const maxSegs = Math.max(1, Math.floor(dataCap() / plotW));
		zoomF = Math.min(Math.max(zoomF, 1), maxSegs);
		segsPerCol = Math.max(1, Math.round(zoomF));
		updateZoomLabel();
	}

	function clampView() {
		const availStart = dataStart();
		const plotW = Math.max(1, W - SCALE_W);
		const minViewEnd = availStart + plotW * segsPerCol;
		viewEndSeg = Math.max(minViewEnd, Math.min(totalSegs, viewEndSeg));
	}

	function panByPx(dx: number) {
		viewEndSeg -= dx * segsPerCol;
		clampView();
		following = viewEndSeg >= totalSegs;
		markDirty();
	}

	function zoomAt(px: number, factor: number) {
		if (!pan) {
			// Monitor mode: no panning, so zoom stays pinned to the live edge.
			zoomF *= factor;
			clampSegs();
			viewEndSeg = totalSegs;
			clampView();
			markDirty();
			return;
		}
		const plotW = Math.max(1, W - SCALE_W);
		const x = Math.min(Math.max(px - SCALE_W, 0), plotW);
		const segAtCursor = viewEndSeg - (plotW - x) * segsPerCol;
		zoomF *= factor;
		clampSegs();
		viewEndSeg = segAtCursor + (plotW - x) * segsPerCol;
		clampView();
		following = viewEndSeg >= totalSegs;
		markDirty();
	}

	function resetView() {
		following = true;
		zoomF = DEFAULT_SEGS;
		clampSegs();
		viewEndSeg = totalSegs;
		clampView();
		markDirty();
	}

	function updateZoomLabel() {
		if (defaultSegs <= 0 || segsPerCol <= 0) {
			zoomLabel = '';
			return;
		}
		const level = defaultSegs / segsPerCol;
		zoomLabel = `×${level < 10 ? level.toFixed(1) : level.toFixed(0)}`;
	}

	function zoomBy(factor: number) {
		const plotW = Math.max(1, W - SCALE_W);
		zoomAt(SCALE_W + plotW / 2, factor);
	}

	function zoomIn() {
		zoomBy(1 / 2);
	}

	function zoomOut() {
		zoomBy(2);
	}

	function canScroll() {
		if (!pan) return false;
		const totalScroll = totalSegs - dataStart();
		return totalScroll > (W - SCALE_W) * segsPerCol;
	}

	function scrollbarMetrics() {
		const plotW = Math.max(1, W - SCALE_W);
		const availStart = dataStart();
		const totalScroll = totalSegs - availStart;
		const visibleSegs = plotW * segsPerCol;
		const trackX = SCALE_W;
		const trackW = plotW;
		const thumbW = totalScroll > 0 ? Math.max(10, trackW * Math.min(1, visibleSegs / totalScroll)) : trackW;
		const scrollable = Math.max(1, trackW - thumbW);
		const denom = Math.max(0, totalScroll - visibleSegs);
		const posFrac = denom > 0 ? Math.min(1, Math.max(0, (viewEndSeg - visibleSegs - availStart) / denom)) : 0;
		return {
			plotW,
			availStart,
			totalScroll,
			visibleSegs,
			trackX,
			trackW,
			thumbW,
			scrollable,
			denom,
			thumbX: trackX + posFrac * scrollable
		};
	}

	function scrollbarToPx(x: number) {
		const m = scrollbarMetrics();
		const frac = Math.min(1, Math.max(0, (x - m.trackX - m.thumbW / 2) / m.scrollable));
		viewEndSeg = m.availStart + frac * m.denom + m.visibleSegs;
		clampView();
		following = viewEndSeg >= totalSegs;
		markDirty();
	}

	function scrollbarPanByPx(dx: number) {
		const m = scrollbarMetrics();
		const segsPerTrackPx = m.denom / m.scrollable;
		viewEndSeg -= dx * segsPerTrackPx;
		clampView();
		following = viewEndSeg >= totalSegs;
		markDirty();
	}

	function formatTime(sec: number): string {
		const m = Math.floor(sec / 60);
		const s = sec - m * 60;
		if (m >= 1) return `${m}:${s.toFixed(1).padStart(4, '0')}`;
		return s.toFixed(2);
	}

	function rebuild() {
		const plotW = W - SCALE_W;
		if (plotW <= 0 || channels <= 0 || H <= TIME_H) {
			colsCount = 0;
			ticks = [];
			return;
		}
		const availStart = dataStart();
		const viewStartSeg = viewEndSeg - plotW * segsPerCol;
		// Stable grid anchor: leftmost column's start segment, kept a whole
		// multiple of the column width; `off` absorbs the sub-column remainder
		// so columns only re-bin on a full-column advance.
		const anchor = Math.floor(viewStartSeg / segsPerCol) * segsPerCol;
		off = (viewStartSeg - anchor) / segsPerCol;

		const cols = plotW + 1;
		colsCount = cols;
		if (peaks.length !== channels) {
			peaks = Array.from({ length: channels }, () => new Float32Array(cols));
			troughs = Array.from({ length: channels }, () => new Float32Array(cols));
		}

		for (let c = 0; c < channels; c++) {
			const pk = peaks[c];
			const tr = troughs[c];
			for (let k = 0; k < cols; k++) {
				let seg0 = anchor + k * segsPerCol;
				let seg1 = seg0 + segsPerCol;
				let mn = 0;
				let mx = 0;
				if (seg1 > availStart && seg0 < totalSegs) {
					seg0 = Math.max(seg0, availStart);
					seg1 = Math.min(seg1, totalSegs);
					mn = Infinity;
					mx = -Infinity;
					for (let seg = seg0; seg < seg1; seg++) {
						const env = segEnvelope(seg, c);
						if (env === null) continue;
						if (env[0] < mn) mn = env[0];
						if (env[1] > mx) mx = env[1];
					}
					if (mn === Infinity) {
						mn = 0;
						mx = 0;
					}
				}
				pk[k] = mx;
				tr[k] = mn;
			}
		}

		const targetSec = (90 * segsPerCol * SEG_FRAMES) / sampleRate;
		const steps = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60];
		let step = steps[steps.length - 1];
		for (const s of steps) {
			if (s >= targetSec) {
				step = s;
				break;
			}
		}
		const stepSamples = step * sampleRate;
		const firstSample = Math.ceil((viewStartSeg * SEG_FRAMES) / stepSamples) * stepSamples;
		const outTicks: { x: number; label: string }[] = [];
		for (let s = firstSample; s < (viewStartSeg + plotW * segsPerCol) * SEG_FRAMES; s += stepSamples) {
			outTicks.push({
				x: Math.round(SCALE_W + (s / SEG_FRAMES - viewStartSeg) / segsPerCol),
				label: formatTime(s / sampleRate)
			});
		}
		ticks = outTicks;
	}

	function roundRect(c: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
		const rr = Math.min(r, w / 2, h / 2);
		c.beginPath();
		c.moveTo(x + rr, y);
		c.lineTo(x + w - rr, y);
		c.arcTo(x + w, y, x + w, y + rr, rr);
		c.lineTo(x + w, y + h - rr);
		c.arcTo(x + w, y + h, x + w - rr, y + h, rr);
		c.lineTo(x + rr, y + h);
		c.arcTo(x, y + h, x, y + h - rr, rr);
		c.lineTo(x, y + rr);
		c.arcTo(x, y, x + rr, y, rr);
		c.closePath();
	}

	function laneMetrics() {
		const laneH = (H - TIME_H) / channels;
		const halfH = Math.max(3, laneH / 2 - Math.min(VERT_PAD, laneH * 0.25));
		return { laneH, halfH };
	}

	function draw() {
		const c = ctx;
		if (!c) return;
		const dpr = window.devicePixelRatio || 1;
		const bw = Math.max(1, Math.round(W * dpr));
		const bh = Math.max(1, Math.round(H * dpr));
		if (canvas.width !== bw) canvas.width = bw;
		if (canvas.height !== bh) canvas.height = bh;
		if (canvas.style.width !== `${W}px`) canvas.style.width = `${W}px`;
		if (canvas.style.height !== `${H}px`) canvas.style.height = `${H}px`;
		c.setTransform(dpr, 0, 0, dpr, 0, 0);
		c.clearRect(0, 0, W, H);

		roundRect(c, 0, 0, W, H, 10);
		c.fillStyle = '#111';
		c.fill();

		if (colsCount > 0 && W > 0) {
			const { laneH, halfH } = laneMetrics();
			for (let i = 0; i < channels; i++) {
				const top = TIME_H + i * laneH;
				const mid = top + laneH / 2;
				const color = channelColor(i);
				const pk = peaks[i];
				const tr = troughs[i];

				c.strokeStyle = 'rgba(255,255,255,0.12)';
				c.lineWidth = 1;
				c.beginPath();
				c.moveTo(SCALE_W, mid);
				c.lineTo(W, mid);
				c.stroke();

				if (pk) {
					c.beginPath();
					c.moveTo(SCALE_W - off, mid - pk[0] * halfH);
					for (let k = 1; k < colsCount; k++) c.lineTo(SCALE_W + k - off, mid - pk[k] * halfH);
					for (let k = colsCount - 1; k >= 0; k--) c.lineTo(SCALE_W + k - off, mid - tr[k] * halfH);
					c.closePath();
					c.fillStyle = color;
					c.globalAlpha = 0.7;
					c.fill();
					c.globalAlpha = 1;
					c.strokeStyle = color;
					c.lineWidth = 0.75;
					c.lineJoin = 'round';
					c.stroke();
				}

				for (const [amp, label] of SCALE_LEVELS) {
					const sy = mid - amp * halfH;
					c.fillStyle = 'rgba(255,255,255,0.2)';
					c.fillRect(SCALE_W - 3, sy - 0.5, 3, 1);
					c.fillStyle = amp === 0 ? 'rgba(255,255,255,0.75)' : 'rgba(255,255,255,0.45)';
					c.font = '7.5px monospace';
					c.textAlign = 'right';
					c.textBaseline = 'middle';
					c.fillText(label, SCALE_W - 5, sy);
				}

				c.fillStyle = color;
				c.font = 'bold 8px monospace';
				c.textAlign = 'left';
				c.fillText(channelLabel(i, channels), SCALE_W + 4, top + 9);

				if (i < channels - 1) {
					c.fillStyle = 'rgba(255,255,255,0.08)';
					c.fillRect(0, top + laneH, W, 1);
				}
			}

			c.strokeStyle = 'rgba(255,255,255,0.14)';
			c.lineWidth = 1;
			c.beginPath();
			c.moveTo(SCALE_W, TIME_H - 1);
			c.lineTo(W, TIME_H - 1);
			c.stroke();

			for (const t of ticks) {
				c.fillStyle = 'rgba(255,255,255,0.07)';
				c.fillRect(t.x, TIME_H, 1, H - TIME_H);
				c.fillStyle = 'rgba(255,255,255,0.6)';
				c.font = '7.5px monospace';
				c.textAlign = 'left';
				c.textBaseline = 'middle';
				c.fillText(t.label, t.x + 3, TIME_H / 2);
			}

			if (canScroll()) {
				const m = scrollbarMetrics();
				const sbTop = H - 8;
				c.fillStyle = 'rgba(0,0,0,0.85)';
				c.fillRect(m.trackX, sbTop, m.trackW, 8);
				c.fillStyle = 'rgba(255,255,255,0.12)';
				c.fillRect(m.trackX, H - 4, m.trackW, 2);
				c.fillStyle = 'rgba(255,255,255,0.45)';
				roundRect(c, m.thumbX, sbTop + 1, m.thumbW, 5, 2.5);
				c.fill();
			}
		}
	}

	async function fetchPeaks(startSeg: number) {
		if (!filePath || fetching) return;
		fetching = true;
		try {
			const plotW = Math.max(1, W - SCALE_W);
			const count = fileLoaded ? Math.max(64, Math.ceil(plotW * segsPerCol) + 32) : 64;
			const res = await methods.readFilePeaks(filePath, startSeg * SEG_FRAMES, SEG_FRAMES, count);
			if (res.channels > 0) {
				fileChannels = res.channels;
				channels = res.channels;
				sampleRate = res.sampleRate;
			}
			fileTotalSegs = Math.ceil(res.totalFrames / SEG_FRAMES);
			fileLoaded = true;
			const firstSeg = Math.floor(res.startFrame / SEG_FRAMES);
			const bins = res.mins[0]?.length ?? 0;
			for (let b = 0; b < bins; b++) {
				const seg = firstSeg + b;
				const arr = new Float32Array(res.channels * 2);
				for (let c = 0; c < res.channels; c++) {
					arr[c * 2] = res.mins[c][b];
					arr[c * 2 + 1] = res.maxs[c][b];
				}
				fileCache.set(seg, arr);
			}
			if (!zoomInit) {
				zoomInit = true;
				zoomF = DEFAULT_SEGS;
				defaultSegs = DEFAULT_SEGS;
				segsPerCol = DEFAULT_SEGS;
				updateZoomLabel();
			}
			totalSegs = fileTotalSegs;
			if (following) viewEndSeg = fileTotalSegs;
			clampSegs();
			clampView();
			markDirty();
		} catch {
			// Unreadable / non-PCM file: leave the scope empty.
		} finally {
			fetching = false;
		}
	}

	function ensureVisibleLoaded() {
		if (!fileMode || !filePath || fetching) return;
		if (!fileLoaded) {
			fetchPeaks(0);
			return;
		}
		const plotW = Math.max(1, W - SCALE_W);
		if (following) {
			// Follow a recording's tail: refresh periodically to catch a growing
			// file (scope events arrive every tick, so throttle the disk read).
			const now = performance.now();
			if (now - lastTailCheck < 500) return;
			lastTailCheck = now;
			fetchPeaks(Math.max(0, fileTotalSegs - Math.ceil(plotW * segsPerCol) - 32));
			return;
		}
		const viewStart = Math.max(0, Math.floor(viewEndSeg - plotW * segsPerCol));
		const viewEnd = Math.ceil(viewEndSeg);
		for (let seg = viewStart; seg < viewEnd; seg++) {
			if (!fileCache.has(seg)) {
				fetchPeaks(seg);
				return;
			}
		}
	}

	// Coalesced repaint: redraw at most once per animation frame, and only when
	// something actually changed. A perpetually-running RAF (or a canvas painted
	// every frame regardless of state) keeps the layer permanently compositing,
	// which on WKWebView degrades anti-aliasing of the surrounding UI — the
	// persistent "everything is slightly aliased while recording" artifact.
	function markDirty() {
		dirty = true;
		if (rafScheduled) return;
		rafScheduled = true;
		rafId = requestAnimationFrame(() => {
			rafScheduled = false;
			dirty = false;
			rebuild();
			draw();
			ensureVisibleLoaded();
		});
	}

	function onWheel(e: WheelEvent) {
		e.preventDefault();
		const rect = wrap.getBoundingClientRect();
		const x = e.clientX - rect.left;
		if (pan && e.shiftKey) {
			panByPx(e.deltaY);
		} else {
			zoomAt(x, Math.exp(-e.deltaY * 0.0015));
		}
	}

	function onDown(e: MouseEvent) {
		if (!pan) {
			e.preventDefault();
			return;
		}
		const rect = wrap.getBoundingClientRect();
		const y = e.clientY - rect.top;
		if (y >= H - SCROLLBAR_HIT && canScroll()) {
			scrollbarDragging = true;
			lastX = e.clientX;
			scrollbarToPx(e.clientX - rect.left);
			e.preventDefault();
			return;
		}
		dragging = true;
		lastX = e.clientX;
		e.preventDefault();
	}

	function onMove(e: MouseEvent) {
		if (scrollbarDragging) {
			const dx = e.clientX - lastX;
			lastX = e.clientX;
			scrollbarPanByPx(dx);
			return;
		}
		if (!dragging) return;
		const dx = e.clientX - lastX;
		lastX = e.clientX;
		panByPx(dx);
	}

	function onUp() {
		dragging = false;
		scrollbarDragging = false;
	}

	function onDblClick() {
		resetView();
	}

	let unlisten: UnlistenFn | undefined;
	let ro: ResizeObserver | undefined;

	// Reset and load from disk when a WAV/AIFF path is set or changes.
	$effect(() => {
		const p = filePath;
		if (!isPcm(p)) return;
		fileCache.clear();
		fileLoaded = false;
		fileTotalSegs = 0;
		fileChannels = 0;
		totalSegs = 0;
		viewEndSeg = 0;
		following = true;
		markDirty();
	});

	onMount(async () => {
		ctx = canvas.getContext('2d');
		unlisten = await listen<ScopeTick>('audio://scope', (e) => onScope(e.payload));
		ro = new ResizeObserver((entries) => {
			const rect = entries[0].contentRect;
			const w = rect.width;
			if (w > 0 && w !== W) {
				W = w;
				clampSegs();
				clampView();
				markDirty();
			}
			const h = rect.height;
			if (h > 0 && h !== H) {
				H = h;
				markDirty();
			}
		});
		ro.observe(wrap);
		markDirty();
	});

	onDestroy(() => {
		unlisten?.();
		ro?.disconnect();
		if (rafId) cancelAnimationFrame(rafId);
	});
</script>

<svelte:window onmousemove={onMove} onmouseup={onUp} />

<div
	bind:this={wrap}
	class="nodrag nopan nowheel relative w-full overflow-hidden"
	class:cursor-grab={pan && !dragging}
	class:cursor-grabbing={pan && dragging}
	style={fill ? 'height:100%' : `height:${H}px`}
	role="img"
	aria-label="Live waveform"
	onwheel={onWheel}
	onmousedown={onDown}
	ondblclick={onDblClick}>
	<canvas bind:this={canvas} style="display:block;width:100%;height:100%" aria-hidden="true"></canvas>
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div
		class="nodrag nopan absolute top-0.5 right-0.5 flex items-center gap-0 rounded bg-neutral-900/40 px-0.5 py-0.5"
		onmousedown={(e) => e.stopPropagation()}
		onwheel={(e) => e.stopPropagation()}
		ondblclick={(e) => e.stopPropagation()}>
		<button
			type="button"
			class="flex size-3.5 items-center justify-center rounded text-[10px] leading-none text-white/60 hover:bg-white/10 hover:text-white"
			onclick={zoomOut}
			title="Zoom out">−</button>
		<span class="min-w-7 text-center font-mono text-[8px] text-white/70 tabular-nums">{zoomLabel}</span>
		<button
			type="button"
			class="flex size-3.5 items-center justify-center rounded text-[10px] leading-none text-white/60 hover:bg-white/10 hover:text-white"
			onclick={zoomIn}
			title="Zoom in">+</button>
	</div>
</div>
