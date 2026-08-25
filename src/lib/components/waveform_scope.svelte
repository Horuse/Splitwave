<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { tauriListen } from '$lib/utils/tauri_event';
	import { channelColor, channelLabel } from '$lib/modules/flow/utils';
	import { methods } from '$lib/modules/audio/methods';
	import { Add, Minus, ChevronDoubleRight } from '$lib/components/icons';

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
	// Upper bound on one peak read, so zooming out to the whole file loads it in
	// chunks instead of one huge read that stalls history while `fetching`.
	const MAX_FETCH_SEGS = 65536;
	// Extra view-widths of disk cache warmed on both sides of the visible range,
	// so panning into neighbouring history hits the cache instead of showing
	// progressive fills. Point fetches only — a whole-file pre-pass would read
	// gigabytes on long recordings (24 h WAV32 ≈ 13 GB).
	const PREFETCH_PLOTS = 1;
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
		// Absolute frame of the block's first sample in the scope's timeline,
		// used to align the live ring exactly with the disk's SEG_FRAMES grid.
		startFrame?: number;
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
	let totalSegs = $state(0);

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
	let fileTotalSegs = $state(0);
	let fileChannels = 0;
	let fileLoaded = $state(false);
	let fetching = false;
	let liveTotalSegs = 0;
	let liveBaseSeg = -1;
	let liveActive = $state(false);
	// Grid-aligned live binning: the total session frames seen and the absolute
	// grid segment currently being accumulated, so the live ring shares the same
	// SEG_FRAMES grid as the disk-loaded bins — no drift/time gap at the seam.
	let liveSessionFrames = 0;
	let liveOpenAbsSeg = -1;
	// Forces the next live-ring init to use a specific base (0 for an overwrite
	// restart) instead of the disk-backed total, which may have regrown.
	let liveBaseOverride: number | null = null;
	// Set when a recording reports `stopped`; the next session that starts with
	// a smaller total (overwrite of the same path) triggers a file-state reset.
	let afterStop = false;
	// When a `stopped` isn't followed by a fresh session (permanent stop), this
	// timer restores the recorded file view that the loader temporarily cleared.
	let stopTimer: ReturnType<typeof setTimeout> | undefined;

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
		// File mode: the disk cache serves the history, so the live ring only
		// keeps a fixed realtime tail. Growing it to the visible span would
		// retain the whole zoomed-out view as realtime bins and blank the
		// unwritten remainder with zeros — a region the disk can't fill because
		// the ring wins the draw.
		const needed = fileMode ? BASE_CAP_SEGS : Math.max(BASE_CAP_SEGS, Math.ceil(plotW * segsPerCol));
		if (needed > capSegs || needed < capSegs / 2) {
			resizeRings(needed);
		}
	}

	function resizeRings(newCap: number) {
		const oldCap = capSegs;
		const count = fileMode ? liveTotalSegs : totalSegs;
		const keep = Math.min(oldCap, newCap, count);
		const newMin = new Float32Array(newCap * channels);
		const newMax = new Float32Array(newCap * channels);
		for (let i = 0; i < keep; i++) {
			let src: number;
			let dst: number;
			if (fileMode) {
				// Grid-aligned live ring is keyed by absolute segment.
				const absSeg = liveBaseSeg + (liveTotalSegs - 1 - i);
				src = ((absSeg % oldCap) + oldCap) % oldCap;
				dst = ((absSeg % newCap) + newCap) % newCap;
			} else {
				src = (((writeSeg - 1 - i) % oldCap) + oldCap) % oldCap;
				const idx = totalSegs - 1 - i;
				dst = ((idx % newCap) + newCap) % newCap;
			}
			newMin.set(minRing.subarray(src * channels, (src + 1) * channels), dst * channels);
			newMax.set(maxRing.subarray(src * channels, (src + 1) * channels), dst * channels);
		}
		capSegs = newCap;
		minRing = newMin;
		maxRing = newMax;
		writeSeg = totalSegs % newCap;
	}

	let W = $state(0);
	let H = $state(height);

	// Draw-state produced by `rebuild` and consumed by `draw` (plain, non-reactive).
	let colsCount = 0;
	let peaks: Float32Array[] = [];
	let troughs: Float32Array[] = [];
	// File mode only: 1 when a column still has uncached segments inside the
	// file's flushed range, so the draw pass can shade it as "loading".
	let missing: Uint8Array = new Uint8Array(0);
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
					// Ring is keyed by absolute segment (grid-aligned), so read
					// the slot by `seg`, not the relative `li`.
					const slot = ((seg % capSegs) + capSegs) % capSegs;
					return [minRing[slot * channels + c], maxRing[slot * channels + c]];
				}
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
		liveTotalSegs = 0;
		liveSessionFrames = 0;
		liveOpenAbsSeg = -1;
		// On a ring reset anchor the live overlay at the current recorded total
		// so a channel/mode change keeps the tail positioned and the view can
		// advance at scope cadence immediately. An explicit override (overwrite
		// restart) wins over the disk-backed total.
		liveBaseSeg = liveBaseOverride !== null ? liveBaseOverride : fileTotalSegs;
		liveBaseOverride = null;
		applyMinHeight();
	}

	// The absolute base of the live session is set in `onScope` before the
	// first bin and on `ensureLiveRing`; it never needs later adjustment.

	// Bins one live block into the ring, aligned to the *absolute* SEG_FRAMES
	// grid so the live tail and the disk-loaded bins cover identical frame
	// ranges. `sessionStartFrame` is the scope-reported absolute frame of the
	// block's first sample; binning from it keeps the ring exactly aligned even
	// if a block is dropped. A segment straddling two blocks is merged by
	// reading back the open slot.
	function binLiveGrid(data: number[][], ch: number, frames: number, sessionStartFrame: number): void {
		for (let f = 0; f < frames; ) {
			const sFrame = sessionStartFrame + f;
			const gridIdx = Math.floor(sFrame / SEG_FRAMES);
			const absSeg = liveBaseSeg + gridIdx;
			// Exclusive index within the block where this grid segment ends.
			const segEnd = (gridIdx + 1) * SEG_FRAMES - sessionStartFrame;
			const f1 = Math.min(segEnd, frames);
			const slot = (((absSeg % capSegs) + capSegs) % capSegs) * ch;
			const fresh = absSeg !== liveOpenAbsSeg;
			for (let c = 0; c < ch; c++) {
				let mn = fresh ? Infinity : minRing[slot + c];
				let mx = fresh ? -Infinity : maxRing[slot + c];
				for (let i = f; i < f1; i++) {
					const v = data[c][i];
					if (v < mn) mn = v;
					if (v > mx) mx = v;
				}
				if (mn === Infinity) {
					mn = 0;
					mx = 0;
				}
				minRing[slot + c] = mn;
				maxRing[slot + c] = mx;
			}
			if (fresh) {
				liveTotalSegs = absSeg - liveBaseSeg + 1;
				liveOpenAbsSeg = absSeg;
			}
			f = f1;
		}
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
			// A `stopped` followed by fresh scope data means the recorder
			// restarted to a fresh file (mode switch in overwrite/new). Drop the
			// old file-backed state now, before the new data anchors.
			if (afterStop) {
				if (stopTimer) {
					clearTimeout(stopTimer);
					stopTimer = undefined;
				}
				fileCache.clear();
				scanCleanKey = '';
				fileLoaded = false;
				fileTotalSegs = 0;
				totalSegs = 0;
				viewEndSeg = 0;
				following = true;
				liveBaseOverride = 0;
				afterStop = false;
			}
			const frames = p.data[0]?.length ?? 0;
			if (frames === 0) {
				if (following) markDirty();
				return;
			}
			ensureLiveRing(p.channels);
			// Establish the absolute base before the first bin so grid-aligned
			// segment indices are correct from the very first block.
			if (liveBaseSeg < 0) liveBaseSeg = Math.max(0, fileTotalSegs);
			liveActive = true;
			const startFrame = p.startFrame ?? liveSessionFrames;
			binLiveGrid(p.data, channels, frames, startFrame);
			liveSessionFrames += frames;
			if (liveBaseSeg >= 0) {
				// The live overlay is the source of truth for the recording's
				// tail. Advance the total at scope cadence so the ruler and the
				// right edge move smoothly; `fileTotalSegs` stays the flushed
				// disk total so history loading never reaches into unflushed
				// (live-only) frames.
				const rt = liveBaseSeg + liveTotalSegs;
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
		if (dx > 0) lastPanDir = 1;
		else if (dx < 0) lastPanDir = -1;
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

	function scrollbarPanByPx(dx: number) {
		// Dragging the thumb pans content 1:1 with the cursor, like the body, so
		// a pixel moves the same distance regardless of file length.
		viewEndSeg -= dx * segsPerCol;
		clampView();
		following = viewEndSeg >= totalSegs;
		if (dx > 0) lastPanDir = 1;
		else if (dx < 0) lastPanDir = -1;
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
		if (missing.length !== cols) missing = new Uint8Array(cols);

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
					missing[k] = 0;
					mn = Infinity;
					mx = -Infinity;
					for (let seg = seg0; seg < seg1; seg++) {
						const env = segEnvelope(seg, c);
						if (env === null) {
							if (c === 0) missing[k] = 1;
							continue;
						}
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
		// Floor, not ceil: one tick before the view start is kept so a label
		// straddling the left boundary keeps rendering (sliced by the draw
		// clip) instead of vanishing whole the moment its anchor crosses.
		const firstSample = Math.floor((viewStartSeg * SEG_FRAMES) / stepSamples) * stepSamples;
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

	// Grows (or shrinks back) the fixed-height widget so every channel lane
	// stays at least `MIN_LANE_H` tall. Fill mode (parent-driven height) and the
	// base `height` are the floor, so dropping from many lanes (multi) back to
	// mono doesn't leave a stretched, full-height waveform.
	function applyMinHeight() {
		if (fill) return;
		const need = TIME_H + channels * MIN_LANE_H;
		const next = Math.max(height, need);
		if (next !== H) H = next;
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
					// Clip to this channel's lane: float recordings can hold
					// amplitudes above 1.0, and an unclipped envelope would draw
					// over the neighbouring lanes.
					c.beginPath();
					c.rect(SCALE_W, top, W - SCALE_W, laneH);
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
					// Shade columns whose disk bins haven't arrived yet, so
					// progressive loading reads as a background fill, not a gap.
					if (fileMode) {
						c.fillStyle = 'rgba(255,255,255,0.04)';
						for (let k = 0; k < colsCount; k++) {
							if (missing[k]) c.fillRect(x0 - k - 0.5, top + 2, 1, laneH - 4);
						}
					}
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

			// Clip to the plot area: a label crossing the left boundary is
			// sliced mid-glyph instead of vanishing whole at the edge.
			c.save();
			c.beginPath();
			c.rect(SCALE_W, 0, W - SCALE_W, H);
			c.clip();
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
			c.restore();

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
		if (!filePath || fetching || disposed) return;
		fetching = true;
		try {
			const plotW = Math.max(1, W - SCALE_W);
			const cnt = Math.min(count ?? (fileLoaded ? Math.max(64, Math.ceil(plotW * segsPerCol) + 32) : 64), MAX_FETCH_SEGS);
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
				// Bins that start at or past the file's frame count are the
				// zeroed tail of an unflushed read; caching them as silence
				// would blank those segments once the flush catches up.
				if (seg * SEG_FRAMES >= res.totalFrames) continue;
				const arr = new Float32Array(res.channels * 2);
				for (let c = 0; c < res.channels; c++) {
					arr[c * 2] = res.mins[c][b];
					arr[c * 2 + 1] = res.maxs[c][b];
				}
				fileCache.set(seg, arr);
			}
			trimFileCache();
			scanCleanKey = '';
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

	// Loads the visible history in capped chunks, one fetch at a time (the
	// `fetching` guard paces it). Chunks are picked in pan-direction order:
	// the margin band ahead of the drag first, then the visible range from the
	// leading edge inward, then the trailing band — so a pan lands on cached
	// history instead of filling under the cursor. A fetch always reads
	// forward, which lines up with the leading-start band; on the other
	// direction the nearest miss sits at the right edge and its chunk warms
	// the margin ahead of it. The clean-scan key skips the O(span) rescan
	// while idle — only view movement or new data re-arms it.
	let scanCleanKey = '';
	let lastPanDir = 0; // +1: view moved to earlier audio, -1: to later
	function firstMissing(from: number, to: number, step: 1 | -1): number {
		if (step > 0) {
			for (let seg = from; seg < to; seg++) if (!fileCache.has(seg)) return seg;
		} else {
			for (let seg = from; seg > to; seg--) if (!fileCache.has(seg)) return seg;
		}
		return -1;
	}
	function ensureVisibleLoaded() {
		if (!fileMode || !filePath || fetching) return;
		if (!fileLoaded) {
			fetchPeaks(0);
			return;
		}
		const plotW = Math.max(1, W - SCALE_W);
		const spanSegs = Math.ceil(plotW * segsPerCol);
		const viewStart = Math.max(0, Math.floor(viewEndSeg - spanSegs));
		// Warm the visible history from disk. Capped at the flushed total: while
		// following, the newest segments are drawn from the live ring, but their
		// disk bins are still loaded so the recorded file is already cached when
		// the ring is torn down on stop. Segments the flush hasn't reached are
		// left to the ring and refetched from the finalized file after stop.
		const viewEnd = Math.min(Math.ceil(viewEndSeg), Math.ceil(fileTotalSegs));
		const margin = spanSegs * PREFETCH_PLOTS;
		const lo = Math.max(0, viewStart - margin);
		const hi = Math.min(Math.ceil(fileTotalSegs), viewEnd + margin);
		if (lo >= hi) return;
		const key = `${lo}|${hi}`;
		if (key === scanCleanKey) return;
		const vs = Math.max(viewStart, lo);
		const ve = Math.min(viewEnd, hi);
		const bands: [number, number, 1 | -1][] =
			lastPanDir > 0
				? [
						[lo, vs, 1],
						[vs, ve, 1],
						[ve, hi, 1]
					]
				: [
						[hi - 1, ve - 1, -1],
						[ve - 1, vs - 1, -1],
						[vs - 1, lo - 1, -1]
					];
		for (const [a, b, step] of bands) {
			const seg = firstMissing(a, b, step);
			if (seg >= 0) {
				fetchPeaks(seg);
				return;
			}
		}
		scanCleanKey = key;
	}

	// Coalesced repaint: redraw at most once per animation frame, and only when
	// something actually changed. A perpetually-running RAF (or a canvas painted
	// every frame regardless of state) keeps the layer permanently compositing,
	// which on WKWebView degrades anti-aliasing of the surrounding UI — the
	// persistent "everything is slightly aliased while recording" artifact.
	function markDirty() {
		if (disposed) return;
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
			// Pressing the track starts a drag, not a jump: only thumb movement
			// pans (1:1 with the cursor). A click on empty space near the bottom
			// must not throw the view across the file.
			scrollbarDragging = true;
			lastX = e.clientX;
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

	let ro: ResizeObserver | undefined;
	// Set on destroy; the mount-time listeners resolve asynchronously, so a
	// fast route change would otherwise leave them registered forever, with
	// every tick waking the dead component (derived_inert flood, dead UI).
	let disposed = false;

	// Reset and load from disk when a WAV/AIFF path is set or changes.
	$effect(() => {
		const p = filePath;
		if (!isPcm(p)) return;
		fileCache.clear();
		scanCleanKey = '';
		fileLoaded = false;
		fileTotalSegs = 0;
		fileChannels = 0;
		totalSegs = 0;
		viewEndSeg = 0;
		following = true;
		liveTotalSegs = 0;
		liveSessionFrames = 0;
		liveOpenAbsSeg = -1;
		liveBaseSeg = -1;
		liveActive = false;
		markDirty();
	});

	// File mode only: recorder progress carries the real-time total (base +
	// session), which the live overlay uses to advance the tail between disk
	// reads, and `stopped` hands the tail back to disk for the final state.
	function onProgress(p: RecorderProgress) {
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
				scanCleanKey = '';
				fileLoaded = false;
				fileTotalSegs = 0;
				totalSegs = 0;
				viewEndSeg = 0;
				following = true;
				liveActive = false;
				liveTotalSegs = 0;
				liveSessionFrames = 0;
				liveOpenAbsSeg = -1;
				liveBaseSeg = 0;
				liveBaseOverride = 0;
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
			liveSessionFrames = 0;
			liveOpenAbsSeg = -1;
			liveBaseSeg = -1;
			// Clear the view so a loader shows during a restart gap instead
			// of the stale wave. If no fresh session follows (permanent
			// stop), the timer restores the recorded file from the intact
			// cache below.
			totalSegs = 0;
			viewEndSeg = 0;
			following = true;
			if (stopTimer) clearTimeout(stopTimer);
			stopTimer = setTimeout(() => {
				stopTimer = undefined;
				totalSegs = fileTotalSegs;
				viewEndSeg = fileTotalSegs;
				markDirty();
			}, 800);
			markDirty();
		}
	}

	onMount(() => {
		ctx = canvas.getContext('2d');
		tauriListen<ScopeTick>('audio://scope', (p) => onScope(p));
		tauriListen<RecorderProgress>('audio://recorder_progress', (p) => onProgress(p));
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
		disposed = true;
		if (stopTimer) clearTimeout(stopTimer);
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
	{#if fileMode && totalSegs <= 0}
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
			class="flex size-3.5 items-center justify-center rounded text-white/60 hover:bg-white/10 hover:text-white"
			onclick={zoomOut}
			title="Zoom out"><Minus class="size-2.5" /></button>
		<span class="min-w-7 text-center font-mono text-[8px] text-white/70 tabular-nums">{zoomLabel}</span>
		<button
			type="button"
			class="flex size-3.5 items-center justify-center rounded text-white/60 hover:bg-white/10 hover:text-white"
			onclick={zoomIn}
			title="Zoom in"><Add class="size-2.5" /></button>
	</div>
	{#if pan && !following}
		<button
			type="button"
			class="nodrag nopan absolute top-7 right-1 flex size-5 items-center justify-center rounded-full bg-neutral-900/40 text-white/70 hover:bg-neutral-900/60 hover:text-white"
			onmousedown={(e) => e.stopPropagation()}
			onwheel={(e) => e.stopPropagation()}
			ondblclick={(e) => e.stopPropagation()}
			onclick={jumpToEnd}
			title="Jump to live edge"><ChevronDoubleRight class="size-3" /></button>
	{/if}
</div>
