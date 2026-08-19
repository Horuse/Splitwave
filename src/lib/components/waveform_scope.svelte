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
		filePath = null,
		maxChannels = null
	}: {
		nodeId: string;
		height?: number;
		fill?: boolean;
		pan?: boolean;
		filePath?: string | null;
		// Caps the number of displayed lanes; extra scope channels (e.g. a
		// phantom multi lane with no cable) are dropped.
		maxChannels?: number | null;
	} = $props();

	const SEG_FRAMES = 64;
	// Ring capacity floor (~6.4 s of history at 48 kHz); the live ring grows on
	// demand to cover the visible span so a wide node zoomed out stays filled.
	const BASE_CAP_SEGS = (300 * 1024) / SEG_FRAMES;
	// Time-label fade zone near the right edge (px).
	const FADE_PX = 40;
	const DEFAULT_SEGS = 20; // fixed "×1" reference, so max zoom (1 seg/px) reads ×20 at any sample rate
	// Fixed zoom steps (label × values), snapped to so the readout never shows
	// arbitrary fractions. Min 0.1 caps the per-column segment count, which also
	// bounds the aggregation cost that made long files janky.
	const ZOOM_LEVELS = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1, 2, 3, 4, 5, 7.5, 10, 15, 20];
	const MAX_FILE_CACHE_SEGS = 200_000;
	const TIME_H = 18;
	const SCALE_W = 30;
	const VERT_PAD = 10;
	// Minimum height per channel lane, so many lanes don't collapse to a
	// squished sliver. The widget grows (non-fill mode) to fit `channels`.
	const MIN_LANE_H = 72;
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

	interface RecorderProgress {
		nodeId: string;
		frames: number;
		sampleRate: number;
		stopped?: boolean;
	}

	let channels = 1;
	let sampleRate = 48_000;
	let segsPerCol = DEFAULT_SEGS;
	let zoomLevelF = 1; // continuous zoom (×) accumulator
	let zoomLevel = 1; // snapped × level shown in the readout
	let viewEndSeg = 0;
	let following = $state(true);

	// Segment min/max rings (block-aligned, immutable once written).
	let capSegs = BASE_CAP_SEGS;
	let minRing = new Float32Array(capSegs);
	let maxRing = new Float32Array(capSegs);
	let writeSeg = 0;
	let totalSegs = 0;

	// File mode (WAV/AIFF only): the whole recording is browsable by lazily
	// loading min/max bins from disk for the visible range instead of holding
	// every sample in RAM. While a recording is in flight, scope deltas for the
	// same node are also binned into the ring below and drawn over the file's
	// tail, so following the live edge is real-time instead of lagging disk
	// flushes. `liveBaseSeg` is the file segment of session frame 0, captured
	// once the header and the first delta are known; the real-time total is
	// then `liveBaseSeg + liveTotalSegs`.
	let fileMode = $derived(isPcm(filePath));
	let fileCache = new Map<number, Float32Array>();
	let fileTotalSegs = 0;
	let fileChannels = 0;
	let fileLoaded = $state(false);
	let fetching = false;
	let lastTailCheck = 0;
	let liveTotalSegs = 0;
	let liveWriteSeg = 0;
	let liveBaseSeg = -1;
	let liveActive = false;
	// Forces the next live-ring init to use a specific base (0 for an overwrite
	// restart) instead of the disk-backed total, which may have regrown.
	let liveBaseOverride: number | null = null;
	// Set when a recording reports `stopped`; the next session that starts with
	// a smaller total (overwrite of the same path) triggers a file-state reset.
	let afterStop = false;

	function isPcm(p: string | null | undefined): boolean {
		if (!p) return false;
		const lower = p.toLowerCase();
		return lower.endsWith('.wav') || lower.endsWith('.aiff') || lower.endsWith('.aif');
	}

	function dataStart(): number {
		return fileMode ? 0 : Math.max(0, totalSegs - capSegs);
	}

	// Grows/shrinks the ring to the visible span so zooming out on a wide node
	// keeps the whole width fed with data. The newest `min(old,new)` segments
	// are preserved at their `index % newCap` slots, so live reads stay correct.
	function ensureCap() {
		const plotW = Math.max(1, W - SCALE_W);
		const needed = Math.max(BASE_CAP_SEGS, Math.ceil(plotW * segsPerCol));
		if (needed > capSegs || needed < capSegs / 2) {
			resizeRings(needed);
		}
	}

	function resizeRings(newCap: number) {
		const oldCap = capSegs;
		const count = fileMode ? liveTotalSegs : totalSegs;
		const head = fileMode ? liveWriteSeg : writeSeg;
		const keep = Math.min(oldCap, newCap, count);
		const newMin = new Float32Array(newCap * channels);
		const newMax = new Float32Array(newCap * channels);
		for (let i = 0; i < keep; i++) {
			const idx = count - 1 - i;
			const src = (((head - 1 - i) % oldCap) + oldCap) % oldCap;
			const dst = ((idx % newCap) + newCap) % newCap;
			newMin.set(minRing.subarray(src * channels, (src + 1) * channels), dst * channels);
			newMax.set(maxRing.subarray(src * channels, (src + 1) * channels), dst * channels);
		}
		capSegs = newCap;
		minRing = newMin;
		maxRing = newMax;
		writeSeg = totalSegs % newCap;
		liveWriteSeg = liveTotalSegs % newCap;
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
	let zoomLabel = $state('×1');
	let lastX = 0;

	function ensureRing(ch: number) {
		const eff = maxChannels ? Math.min(ch, maxChannels) : ch;
		if (eff === channels && minRing.length === capSegs * eff) return;
		channels = eff;
		minRing = new Float32Array(capSegs * eff);
		maxRing = new Float32Array(capSegs * eff);
		writeSeg = 0;
		totalSegs = 0;
		viewEndSeg = 0;
		following = true;
		applyMinHeight();
	}

	function segSlot(d: number): number {
		let slot = (writeSeg - 1 - d) % capSegs;
		if (slot < 0) slot += capSegs;
		return slot;
	}

	function segEnvelope(seg: number, c: number): [number, number] | null {
		if (fileMode) {
			if (liveActive && liveBaseSeg >= 0 && liveTotalSegs > 0) {
				// `liveBaseSeg` is captured once, so the live envelope never
				// re-bins when the disk-backed total advances on flush/progress.
				const li = seg - liveBaseSeg;
				if (li >= 0 && li < liveTotalSegs && li >= liveTotalSegs - capSegs) {
					const slot = li % capSegs;
					return [minRing[slot * channels + c], maxRing[slot * channels + c]];
				}
				// While following the live edge the view is fed purely from the
				// ring; disk would draw the lagging flushed wave and visibly
				// overwrite the realtime tail. History is shown only after the
				// user pans away (following becomes false).
				if (following) return null;
			}
			const e = fileCache.get(seg);
			return e ? [e[c * 2], e[c * 2 + 1]] : null;
		}
		const base = segSlot(totalSegs - 1 - seg) * channels + c;
		return [minRing[base], maxRing[base]];
	}

	function ensureLiveRing(ch: number) {
		const eff = maxChannels ? Math.min(ch, maxChannels) : ch;
		if (eff === channels && minRing.length === capSegs * eff) return;
		channels = eff;
		minRing = new Float32Array(capSegs * eff);
		maxRing = new Float32Array(capSegs * eff);
		liveWriteSeg = 0;
		liveTotalSegs = 0;
		// On a ring reset anchor the live overlay at the current recorded total
		// so a channel/mode change keeps the tail positioned and the view can
		// advance at scope cadence immediately. An explicit override (overwrite
		// restart) wins over the disk-backed total.
		liveBaseSeg = liveBaseOverride !== null ? liveBaseOverride : fileTotalSegs;
		liveBaseOverride = null;
		applyMinHeight();
	}

	function captureLiveBase() {
		if (liveBaseSeg < 0 && fileLoaded && liveTotalSegs > 0) {
			// `Math.max(0, ...)` keeps a fresh (empty) file's base at 0 instead
			// of going negative while the disk total lags the live overlay.
			liveBaseSeg = Math.max(0, fileTotalSegs - liveTotalSegs);
		}
	}

	function liveCoverStart(): number {
		return fileTotalSegs - Math.min(liveTotalSegs, capSegs);
	}

	// Bins one incoming block into the min/max ring, returning the number of
	// segments written. `head` is the write head of whichever ring the caller
	// owns (the scope ring or the live overlay); a segment lands at
	// `index % capSegs`.
	function binBlock(data: number[][], ch: number, frames: number, head: number): number {
		const segsInBlock = Math.max(1, Math.ceil(frames / SEG_FRAMES));
		for (let s = 0; s < segsInBlock; s++) {
			const f0 = s * SEG_FRAMES;
			const f1 = Math.min(f0 + SEG_FRAMES, frames);
			const base = ((head + s) % capSegs) * ch;
			for (let c = 0; c < ch; c++) {
				let mn = Infinity;
				let mx = -Infinity;
				for (let f = f0; f < f1; f++) {
					const v = data[c][f];
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
		return segsInBlock;
	}

	function onScope(p: ScopeTick) {
		if (p.nodeId !== nodeId) return;
		if (fileMode) {
			if (p.sampleRate) sampleRate = p.sampleRate;
			const frames = p.data[0]?.length ?? 0;
			if (frames === 0) {
				if (following) markDirty();
				return;
			}
			ensureLiveRing(p.channels);
			liveActive = true;
			const segs = binBlock(p.data, channels, frames, liveWriteSeg);
			liveWriteSeg = (liveWriteSeg + segs) % capSegs;
			liveTotalSegs += segs;
			captureLiveBase();
			if (liveBaseSeg >= 0) {
				// The live overlay is the source of truth for the recording's
				// tail. Advance the total at scope cadence so the ruler and the
				// right edge move smoothly; gating on `rt > fileTotalSegs` would
				// pause the view for the slower disk/progress ticks.
				const rt = liveBaseSeg + liveTotalSegs;
				fileTotalSegs = Math.max(fileTotalSegs, rt);
				totalSegs = Math.max(totalSegs, rt);
			}
			if (following) viewEndSeg = totalSegs;
			markDirty();
			return;
		}
		ensureRing(p.channels);
		if (p.sampleRate) sampleRate = p.sampleRate;
		const frames = p.data[0]?.length ?? 0;
		if (frames === 0) return;
		const segs = binBlock(p.data, channels, frames, writeSeg);
		writeSeg = (writeSeg + segs) % capSegs;
		totalSegs += segs;
		if (following) viewEndSeg = totalSegs;
		markDirty();
	}

	function clampSegs() {
		zoomLevelF = Math.min(Math.max(zoomLevelF, ZOOM_LEVELS[0]), ZOOM_LEVELS[ZOOM_LEVELS.length - 1]);
		// Snap to the nearest fixed level; the 0.1 floor keeps the per-column
		// aggregation cost bounded. No data-fitting cap: zooming out past the
		// available content just leaves leading empty space, as in any editor.
		let best = ZOOM_LEVELS[0];
		for (const l of ZOOM_LEVELS) {
			if (Math.abs(l - zoomLevelF) < Math.abs(best - zoomLevelF)) best = l;
		}
		zoomLevel = best;
		segsPerCol = Math.max(1, Math.round(DEFAULT_SEGS / zoomLevel));
		ensureCap();
		updateZoomLabel();
	}

	function clampView() {
		const availStart = dataStart();
		const plotW = Math.max(1, W - SCALE_W);
		// Capped at `totalSegs`: a view wider than the available data must keep
		// the live edge, leaving leading empty space, rather than pushing past
		// the end (which would flicker against `following`).
		const minViewEnd = Math.min(availStart + plotW * segsPerCol, totalSegs);
		viewEndSeg = Math.max(minViewEnd, Math.min(totalSegs, viewEndSeg));
	}

	function panByPx(dx: number) {
		viewEndSeg -= dx * segsPerCol;
		clampView();
		following = viewEndSeg >= totalSegs;
		markDirty();
	}

	function zoomAt(px: number, factor: number) {
		if (!pan || following) {
			// Monitor mode, or following the live edge: zoom stays pinned to the
			// edge so the timeline keeps advancing while you zoom.
			zoomLevelF /= factor;
			clampSegs();
			viewEndSeg = totalSegs;
			clampView();
			markDirty();
			return;
		}
		const plotW = Math.max(1, W - SCALE_W);
		const x = Math.min(Math.max(px - SCALE_W, 0), plotW);
		const segAtCursor = viewEndSeg - (plotW - x) * segsPerCol;
		zoomLevelF /= factor;
		clampSegs();
		viewEndSeg = segAtCursor + (plotW - x) * segsPerCol;
		clampView();
		following = viewEndSeg >= totalSegs;
		markDirty();
	}

	function resetView() {
		following = true;
		zoomLevelF = 1;
		clampSegs();
		viewEndSeg = totalSegs;
		clampView();
		markDirty();
	}

	function updateZoomLabel() {
		zoomLabel = `×${Number.isInteger(zoomLevel) ? zoomLevel : zoomLevel.toFixed(1)}`;
	}

	function stepLevel(dir: 1 | -1) {
		let idx = ZOOM_LEVELS.indexOf(zoomLevel);
		if (idx < 0) idx = ZOOM_LEVELS.length - 1;
		idx = Math.min(Math.max(idx + dir, 0), ZOOM_LEVELS.length - 1);
		zoomLevelF = ZOOM_LEVELS[idx];
		clampSegs();
		if (following) {
			viewEndSeg = totalSegs;
		} else {
			const plotW = Math.max(1, W - SCALE_W);
			const x = plotW / 2;
			const segAt = viewEndSeg - (plotW - x) * segsPerCol;
			viewEndSeg = segAt + (plotW - x) * segsPerCol;
		}
		clampView();
		following = viewEndSeg >= totalSegs;
		markDirty();
	}

	function zoomIn() {
		stepLevel(1);
	}

	function zoomOut() {
		stepLevel(-1);
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
		// Dragging the thumb pans content 1:1 with the cursor, like the body, so
		// a pixel moves the same distance regardless of file length.
		viewEndSeg -= dx * segsPerCol;
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
		// Right-anchored stable grid: column boundaries are multiples of
		// `segsPerCol` counted from the view *end*, so the rightmost column
		// always covers the newest audio. A left-anchored grid leaves that
		// column empty whenever the left edge aligns to a boundary (`off === 0`,
		// which is permanent at ×20 where segsPerCol === 1) — a flat notch at
		// the live edge that visibly fills in as the view scrolls.
		const rightAnchor = Math.ceil(viewEndSeg / segsPerCol) * segsPerCol;
		off = (rightAnchor - viewEndSeg) / segsPerCol;

		const cols = plotW + 1;
		colsCount = cols;
		if (peaks.length !== channels || peaks[0]?.length !== cols) {
			peaks = Array.from({ length: channels }, () => new Float32Array(cols));
			troughs = Array.from({ length: channels }, () => new Float32Array(cols));
		}

		for (let c = 0; c < channels; c++) {
			const pk = peaks[c];
			const tr = troughs[c];
			for (let k = 0; k < cols; k++) {
				let seg1 = rightAnchor - k * segsPerCol;
				let seg0 = seg1 - segsPerCol;
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
				// Fractional x so ticks glide with the stream instead of
				// integer-snapping (which read as micro-stutter).
				x: SCALE_W + (s / SEG_FRAMES - viewStartSeg) / segsPerCol,
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

	// Grows the fixed-height widget so every channel lane stays at least
	// `MIN_LANE_H` tall. Fill mode (parent-driven height) is left alone.
	function applyMinHeight() {
		if (fill) return;
		const need = TIME_H + channels * MIN_LANE_H;
		if (need > H) H = need;
	}

	function laneMetrics() {
		const laneH = (H - TIME_H) / channels;
		const halfH = Math.max(3, laneH / 2 - Math.min(VERT_PAD, laneH * 0.25));
		return { laneH, halfH };
	}

	function draw() {
		const c = ctx;
		// `canvas`/`ctx` are nulled on unmount; a rAF or async fetch may still
		// land after teardown, so bail instead of touching a removed element.
		if (!c || !canvas) return;
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
					c.save();
					// Clip to the plot area so the envelope never bleeds into the
					// scale gutter behind the amp labels.
					c.beginPath();
					c.rect(SCALE_W, TIME_H, W - SCALE_W, H - TIME_H);
					c.clip();
					c.beginPath();
					// Right-anchored: x0 is the newest column's centre, at the
					// right edge; index k runs right-to-left. The +0.5 tile keeps
					// the columns covering the full plot at every sub-pixel off.
					const x0 = W - 0.5 + off;
					c.moveTo(x0, mid - pk[0] * halfH);
					for (let k = 1; k < colsCount; k++) c.lineTo(x0 - k, mid - pk[k] * halfH);
					for (let k = colsCount - 1; k >= 0; k--) c.lineTo(x0 - k, mid - tr[k] * halfH);
					c.closePath();
					c.fillStyle = color;
					c.globalAlpha = 0.7;
					c.fill();
					c.globalAlpha = 1;
					c.strokeStyle = color;
					c.lineWidth = 0.75;
					c.lineJoin = 'round';
					c.stroke();
					c.restore();
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
				// Fade tick + label as the label nears the right edge so it glides
				// out instead of being clipped mid-glyph.
				c.font = '7.5px monospace';
				const labelW = c.measureText(t.label).width;
				const fade = Math.max(0, Math.min(1, (W - (t.x + 3 + labelW)) / FADE_PX));
				if (fade <= 0) continue;
				c.fillStyle = `rgba(255,255,255,${0.07 * fade})`;
				c.fillRect(t.x, TIME_H, 1, H - TIME_H);
				c.fillStyle = `rgba(255,255,255,${0.6 * fade})`;
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

	async function fetchPeaks(startSeg: number, count?: number) {
		if (!filePath || fetching) return;
		fetching = true;
		try {
			const plotW = Math.max(1, W - SCALE_W);
			const cnt = count ?? (fileLoaded ? Math.max(64, Math.ceil(plotW * segsPerCol) + 32) : 64);
			const res = await methods.readFilePeaks(filePath, startSeg * SEG_FRAMES, SEG_FRAMES, cnt);
			if (res.channels > 0) {
				fileChannels = res.channels;
				channels = maxChannels ? Math.min(res.channels, maxChannels) : res.channels;
				sampleRate = res.sampleRate;
				applyMinHeight();
			}
			// Never regress: the live overlay / progress events may already know
			// a larger total than the last disk flush.
			fileTotalSegs = Math.max(fileTotalSegs, Math.ceil(res.totalFrames / SEG_FRAMES));
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
			trimFileCache();
			captureLiveBase();
			// While following the live edge the scope stream owns the view;
			// moving it here from the (lagging) disk total is what made it step.
			if (!(liveActive && following)) {
				totalSegs = fileTotalSegs;
				if (following) viewEndSeg = fileTotalSegs;
			}
			clampSegs();
			clampView();
			markDirty();
		} catch {
			// Unreadable / non-PCM file: leave the scope empty.
		} finally {
			fetching = false;
		}
	}

	function trimFileCache() {
		if (fileCache.size <= MAX_FILE_CACHE_SEGS) return;
		const keys = [...fileCache.keys()].sort((a, b) => Math.abs(a - viewEndSeg) - Math.abs(b - viewEndSeg));
		while (fileCache.size > MAX_FILE_CACHE_SEGS) {
			const k = keys.pop();
			if (k === undefined) break;
			fileCache.delete(k);
		}
	}

	function ensureVisibleLoaded() {
		if (!fileMode || !filePath || fetching) return;
		if (!fileLoaded) {
			fetchPeaks(0);
			return;
		}
		const plotW = Math.max(1, W - SCALE_W);
		const liveStart = liveCoverStart();
		if (following) {
			const viewStart = Math.max(0, Math.ceil(viewEndSeg - plotW * segsPerCol));
			// The live overlay already covers the newest ring segments; skip the
			// disk read entirely when the view sits inside it.
			if (liveActive && liveTotalSegs > 0 && viewStart >= liveStart) return;
			const now = performance.now();
			if (now - lastTailCheck < 500) return;
			lastTailCheck = now;
			if (liveActive && liveTotalSegs > 0) {
				fetchPeaks(Math.max(0, viewStart), Math.max(64, liveStart - viewStart));
			} else {
				fetchPeaks(Math.max(0, fileTotalSegs - Math.ceil(plotW * segsPerCol) - 32));
			}
			return;
		}
		const viewStart = Math.max(0, Math.floor(viewEndSeg - plotW * segsPerCol));
		const viewEnd = Math.ceil(viewEndSeg);
		for (let seg = viewStart; seg < viewEnd; seg++) {
			if (liveActive && seg >= liveStart) continue;
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

	function jumpToEnd() {
		following = true;
		viewEndSeg = totalSegs;
		clampView();
		markDirty();
	}

	let unlisten: UnlistenFn | undefined;
	let progressUnlisten: UnlistenFn | undefined;
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
		liveTotalSegs = 0;
		liveWriteSeg = 0;
		liveBaseSeg = -1;
		liveActive = false;
		markDirty();
	});

	onMount(async () => {
		ctx = canvas.getContext('2d');
		unlisten = await listen<ScopeTick>('audio://scope', (e) => onScope(e.payload));
		// File mode only: recorder progress carries the real-time total (base +
		// session), which the live overlay uses to advance the tail between disk
		// reads, and `stopped` hands the tail back to disk for the final state.
		progressUnlisten = await listen<RecorderProgress>('audio://recorder_progress', (e) => {
			const p = e.payload;
			if (p.nodeId !== nodeId || !fileMode) return;
			if (p.sampleRate) sampleRate = p.sampleRate;
			if (p.frames > 0) {
				const segs = Math.max(1, Math.ceil(p.frames / SEG_FRAMES));
				// A fresh session whose total drops below the current one means
				// the file was overwritten (mode "overwrite" rewrites the same
				// path). Drop the old file-backed state so a stale wave and time
				// scale don't linger, then let the new file refill from scratch.
				if (afterStop && segs < fileTotalSegs) {
					fileCache.clear();
					fileLoaded = false;
					fileTotalSegs = 0;
					totalSegs = 0;
					viewEndSeg = 0;
					following = true;
					liveActive = false;
					liveTotalSegs = 0;
					liveWriteSeg = 0;
					liveBaseSeg = 0;
					liveBaseOverride = 0;
					lastTailCheck = 0;
				}
				if (segs > fileTotalSegs) {
					fileTotalSegs = segs;
					// While following the live edge the scope stream owns the
					// view; progress ticks (250 ms) must not step it.
					if (!(liveActive && following)) {
						totalSegs = segs;
						if (following) viewEndSeg = segs;
					}
					markDirty();
				}
				afterStop = false;
			}
			if (p.stopped) {
				afterStop = true;
				liveActive = false;
				liveTotalSegs = 0;
				liveWriteSeg = 0;
				liveBaseSeg = -1;
				lastTailCheck = 0;
				ensureVisibleLoaded();
				markDirty();
			}
		});
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
		progressUnlisten?.();
		ro?.disconnect();
		ctx = null;
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
	{#if fileMode && !fileLoaded}
		<div class="pointer-events-none absolute inset-0 flex items-center justify-center">
			<span class="rounded bg-neutral-900/60 px-1.5 py-0.5 font-mono text-[9px] text-white/60">loading…</span>
		</div>
	{/if}
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
	{#if pan && !following}
		<button
			type="button"
			class="nodrag nopan absolute top-7 right-1 flex size-5 items-center justify-center rounded-full bg-neutral-900/40 text-[11px] leading-none text-white/70 hover:bg-neutral-900/60 hover:text-white"
			onmousedown={(e) => e.stopPropagation()}
			onwheel={(e) => e.stopPropagation()}
			ondblclick={(e) => e.stopPropagation()}
			onclick={jumpToEnd}
			title="Jump to live edge">»</button>
	{/if}
</div>
