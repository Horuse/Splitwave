<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { blur } from 'svelte/transition';
	import { tauriListen } from '$lib/utils/tauri_event';
	import { channelColor, channelLabel } from '$lib/modules/flow/utils';
	import { methods } from '$lib/modules/audio/methods';
	import { Add, Minus, ChevronDoubleRight } from '$lib/components/icons';

	// Canvas waveform viewer shared by the Waveform and File Recording nodes:
	// scope blocks are pre-binned into fixed segments, and the view
	// re-aggregates them per column so the envelope never shimmers while
	// streaming. Repaints are coalesced through rAF.
	let {
		nodeId,
		height = 140,
		fill = false,
		pan = true,
		filePath = null,
		// Encoder writes PCM (WAV/AIFF) — the disk-peak source; `null` falls
		// back to the path-extension heuristic.
		pcm = null,
		maxChannels = null
	}: {
		nodeId: string;
		height?: number;
		fill?: boolean;
		pan?: boolean;
		filePath?: string | null;
		pcm?: boolean | null;
		// Caps displayed lanes; phantom multi lanes without a cable are dropped.
		maxChannels?: number | null;
	} = $props();

	const SEG_FRAMES = 64;
	// Ring capacity floor (~6.4 s of history at 48 kHz); grows on demand to
	// the visible span.
	const BASE_CAP_SEGS = (300 * 1024) / SEG_FRAMES;
	const FADE_PX = 40;
	const DEFAULT_SEGS = 20; // fixed "×1" reference, so max zoom (1 seg/px) reads ×20 at any sample rate
	// Zoom is time-normalized: ×1 covers the same wall-clock window at any
	// sample rate, so nodes at different rates scroll identically.
	const REFERENCE_RATE = 48_000;
	// Fixed zoom steps; the 0.1 floor bounds per-column aggregation cost.
	const ZOOM_LEVELS = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1, 2, 3, 4, 5, 7.5, 10, 15, 20];
	const MAX_FILE_CACHE_SEGS = 200_000;
	const TIME_H = 22;
	const SCROLL_H = 14;
	const SCALE_W = 30;
	const VERT_PAD = 10;
	// Many lanes must not collapse to a sliver; the widget grows to fit.
	const MIN_LANE_H = 72;
	// Caps one peak read so zooming out loads in chunks, not one huge read.
	const MAX_FETCH_SEGS = 65536;
	// Extra view-widths of disk cache warmed around the visible range; point
	// fetches only -- a whole-file pre-pass would read gigabytes.
	const PREFETCH_PLOTS = 1;
	const SCROLLBAR_HIT = SCROLL_H;
	// Live-only formats (MP3/Opus/FLAC/AAC) have no disk peak source: their
	// browsable history is the binned scope stream, kept per node id so it
	// survives remounts. Cleared when a new session rewrites the timeline.
	const liveHistories = new Map<string, { session: number; bins: Map<number, Float32Array>; low: number }>();
	// ~3.5 h per node at 48 kHz; trimmed from the front once exceeded.
	const LIVE_HISTORY_CAP = 200_000;
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
		// Recorder session id and its append base (recorder scopes only). A
		// different id means the previous recording session's state is stale.
		session?: number;
		baseFrames?: number;
	}

	interface RecorderProgress {
		nodeId: string;
		frames: number;
		sampleRate: number;
		stopped?: boolean;
		session?: number;
		baseFrames?: number;
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

	// File mode (WAV/AIFF): the recording is browsable by lazy-loading min/max
	// bins from disk for the visible range; while recording, scope deltas are
	// binned into a ring and drawn over the file's tail. Disk peaks follow the
	// encoder's format, not the path extension; `pcm === null` falls back to
	// the extension heuristic.
	let fileMode = $derived(pcm === null ? isPcm(filePath) : pcm && filePath != null);
	let fileCache = new Map<number, Float32Array>();
	// Progress-driven total; leads the disk by up to one flush interval.
	let fileTotalSegs = $state(0);
	// Total actually readable from disk; scans must cap here -- past it
	// nothing can be cached and the scan would chase it forever.
	let readableTotalSegs = 0;
	let fileChannels = 0;
	let fileLoaded = $state(false);
	let fetching = false;
	let liveTotalSegs = 0;
	let liveBaseSeg = -1;
	let liveActive = $state(false);
	// Grid-aligned live binning: the live ring shares the SEG_FRAMES grid with
	// the disk-loaded bins.
	let liveSessionFrames = 0;
	let liveOpenAbsSeg = -1;
	// First segment the live ring wrote this session; ring reads stop there.
	let liveFirstSeg = -1;
	// Live-only: oldest absolute segment the ring holds; older slots are
	// zeroed after resizes/window slides, so reads fall to the store.
	let ringFrom = -1;
	// Last frame the previous scope tick ended at; a forward jump means the
	// backend ring overwrote frames (a skipped span).
	let liveLastEnd = -1;
	// Between an observed `stopped` and the next session: dying-tail ticks.
	let afterStop = false;
	// Session id currently rendered; newer payloads trigger adoption.
	let session = -1;
	// Shared with other instances of the same node.
	let liveStore = liveHistories.get(nodeId) ?? { session: -1, bins: new Map<number, Float32Array>(), low: -1 };
	liveHistories.set(nodeId, liveStore);

	// Copies one closed segment's ring bins into the history store.
	function storeLiveBin(seg: number, slot: number, ch: number) {
		const arr = new Float32Array(ch * 2);
		for (let c = 0; c < ch; c++) {
			arr[c * 2] = minRing[slot * ch + c];
			arr[c * 2 + 1] = maxRing[slot * ch + c];
		}
		liveStore.bins.set(seg, arr);
		if (liveStore.low < 0) liveStore.low = seg;
		while (liveStore.bins.size > LIVE_HISTORY_CAP && liveStore.low <= seg) {
			liveStore.bins.delete(liveStore.low);
			liveStore.low++;
		}
	}

	// Everything here is stale the moment a new session starts.
	function resetFileState() {
		fileCache.clear();
		scanCleanKey = '';
		fileLoaded = false;
		fileTotalSegs = 0;
		readableTotalSegs = 0;
		totalSegs = 0;
		viewEndSeg = 0;
		following = true;
		liveActive = false;
		liveTotalSegs = 0;
		liveSessionFrames = 0;
		liveOpenAbsSeg = -1;
		liveFirstSeg = -1;
		liveBaseSeg = 0;
	}

	// A fresh session id resets the view for overwrite/new; append keeps it.
	function adoptSession(sid: number, baseFrames: number) {
		session = sid;
		afterStop = false;
		if (stopTimer) {
			clearTimeout(stopTimer);
			stopTimer = undefined;
		}
		liveActive = false;
		liveTotalSegs = 0;
		liveSessionFrames = 0;
		liveOpenAbsSeg = -1;
		liveFirstSeg = -1;
		liveLastEnd = -1;
		liveBaseSeg = 0;
		if (baseFrames <= 0) resetFileState();
	}

	// An overwrite restart clears the shared history; remount/append keeps it.
	function adoptLiveSession(sid: number, baseFrames: number) {
		session = sid;
		if (liveStore.session !== sid && baseFrames <= 0) {
			liveStore.bins = new Map();
			liveStore.low = -1;
		}
		liveStore.session = sid;
		minRing.fill(0);
		maxRing.fill(0);
		writeSeg = 0;
		ringFrom = -1;
		liveLastEnd = -1;
		totalSegs = 0;
		viewEndSeg = 0;
		following = true;
	}
	// When a `stopped` isn't followed by a fresh session (permanent stop), this
	// timer restores the recorded file view that the loader temporarily cleared.
	let stopTimer: ReturnType<typeof setTimeout> | undefined;

	function isPcm(p: string | null | undefined): boolean {
		if (!p) return false;
		const lower = p.toLowerCase();
		return lower.endsWith('.wav') || lower.endsWith('.aiff') || lower.endsWith('.aif');
	}

	function dataStart(): number {
		if (fileMode) return 0;
		// Live-only: bounded by the oldest segment retained in the live store.
		if (liveStore.low >= 0) return liveStore.low;
		if (liveStore.bins.size > 0) return 0;
		return Math.max(0, totalSegs - capSegs);
	}

	// Grows/shrinks the ring to the visible span, preserving the newest
	// segments at their `index % newCap` slots.
	function ensureCap() {
		const plotW = Math.max(1, W - SCALE_W);
		// File mode keeps a fixed realtime tail: growing the ring would blank
		// unwritten zeros over the disk history.
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
		// Slots below the copied range were never rewritten; a resize can only
		// keep what the ring already held, never extend it over zeroed slots.
		if (!fileMode && ringFrom >= 0) ringFrom = Math.max(ringFrom, totalSegs - keep);
	}

	let W = $state(0);
	let H = $state(height);

	// Draw-state produced by `rebuild` and consumed by `draw` (plain, non-reactive).
	let colsCount = 0;
	let peaks: Float32Array[] = [];
	let troughs: Float32Array[] = [];
	// File mode: a column has uncached segments in the flushed range; the draw
	// shades it as "loading".
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
	let scrollbarDragging = $state(false);
	let scrollbarHover = $state(false);
	let zoomLabel = $state('×1');
	let lastX = 0;

	function ensureRing(ch: number) {
		const eff = maxChannels ? Math.min(ch, maxChannels) : ch;
		if (eff === channels && minRing.length === capSegs * eff) return;
		channels = eff;
		minRing = new Float32Array(capSegs * eff);
		maxRing = new Float32Array(capSegs * eff);
		writeSeg = 0;
		ringFrom = -1;
		liveLastEnd = -1;
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
			if (liveActive && liveBaseSeg >= 0 && liveTotalSegs > 0 && (liveFirstSeg < 0 || seg >= liveFirstSeg)) {
				const li = seg - liveBaseSeg;
				if (li >= 0 && li < liveTotalSegs && li >= liveTotalSegs - capSegs) {
					// The ring is keyed by absolute segment, not by `li`.
					const slot = ((seg % capSegs) + capSegs) % capSegs;
					return [minRing[slot * channels + c], maxRing[slot * channels + c]];
				}
			}
			const e = fileCache.get(seg);
			return e ? [e[c * 2], e[c * 2 + 1]] : null;
		}
		if (ringFrom >= 0 && seg >= ringFrom) {
			const d = totalSegs - 1 - seg;
			if (d >= 0 && d < capSegs) {
				const base = segSlot(d) * channels + c;
				return [minRing[base], maxRing[base]];
			}
		}
		// Outside the ring's held range: the shared history store, or no data.
		const e = liveStore.bins.get(seg);
		return e ? [e[c * 2], e[c * 2 + 1]] : null;
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
		liveFirstSeg = -1;
		liveLastEnd = -1;
		// startFrame is file-absolute, so the overlay anchors at segment 0.
		liveBaseSeg = 0;
		applyMinHeight();
	}

	// Bins one live block into the ring aligned to the absolute SEG_FRAMES
	// grid, so the live tail matches the disk bins exactly; a segment
	// straddling two blocks merges by reading back the open slot.
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
				if (liveFirstSeg < 0 || absSeg < liveFirstSeg) liveFirstSeg = absSeg;
			}
			f = f1;
		}
	}

	// Bins one incoming block into the min/max ring at `head`; a segment lands
	// at `index % capSegs`.
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
			setSampleRate(p.sampleRate);
			// Session ids only move forward; a smaller one is a straggler from
			// an already-replaced recording, and an equal one after `stopped`
			// is its dying tail -- neither may touch the current state.
			const sid = p.session ?? 0;
			if (sid < session) return;
			if (sid !== session) adoptSession(sid, p.baseFrames ?? 0);
			else if (afterStop) return;
			const frames = p.data[0]?.length ?? 0;
			if (frames === 0) {
				if (following) markDirty();
				return;
			}
			ensureLiveRing(p.channels);
			liveActive = true;
			const startFrame = p.startFrame ?? liveSessionFrames;
			// A scope-ring overrun skips grid segments; cut the ring at the gap
			// so the skipped span loads from the disk cache instead of showing
			// the ring's stale slots.
			if (liveLastEnd >= 0 && startFrame > liveLastEnd) {
				liveFirstSeg = liveBaseSeg + Math.floor(startFrame / SEG_FRAMES);
			}
			liveLastEnd = startFrame + frames;
			binLiveGrid(p.data, channels, frames, startFrame);
			liveSessionFrames += frames;
			if (liveBaseSeg >= 0) {
				// The live overlay owns the tail total; fileTotalSegs stays the
				// flushed disk total so loading never reaches unflushed frames.
				const rt = liveBaseSeg + liveTotalSegs;
				totalSegs = Math.max(totalSegs, rt);
			}
			if (following) viewEndSeg = totalSegs;
			markDirty();
			return;
		}
		// Live-only scopes (compressed formats have no disk peak source) track
		// the session too: without this an overwrite restart would append the
		// new take onto the old waveform's tail.
		const sid = p.session ?? 0;
		if (p.startFrame !== undefined) {
			if (sid < session) return;
			if (sid !== session) adoptLiveSession(sid, p.baseFrames ?? 0);
		} else if (sid > session) {
			session = sid;
		}
		ensureRing(p.channels);
		setSampleRate(p.sampleRate);
		const frames = p.data[0]?.length ?? 0;
		if (frames === 0) return;
		const head = writeSeg;
		const segs = binBlock(p.data, channels, frames, head);
		writeSeg = (writeSeg + segs) % capSegs;
		// Recorder scopes carry file-absolute frames: mid-session entry pins
		// the live edge to the true recording position.
		totalSegs = p.startFrame !== undefined ? Math.floor((p.startFrame + frames) / SEG_FRAMES) : totalSegs + segs;
		// Closed segments land in the shared history, which survives remounts
		// and appends.
		const first = Math.max(0, totalSegs - segs);
		// A scope-ring overrun skips segments; cut the ring at the gap so the
		// stale slots cannot render as shifted data.
		if (liveLastEnd >= 0 && p.startFrame !== undefined && p.startFrame > liveLastEnd) ringFrom = first;
		liveLastEnd = p.startFrame !== undefined ? p.startFrame + frames : Math.max(0, liveLastEnd) + frames;
		for (let s = 0; s < segs; s++) storeLiveBin(first + s, (head + s) % capSegs, channels);
		ringFrom = ringFrom < 0 ? first : Math.max(ringFrom, totalSegs - capSegs);
		if (following) {
			viewEndSeg = totalSegs;
		} else {
			clampView();
		}
		markDirty();
	}

	function clampSegs() {
		zoomLevelF = Math.min(Math.max(zoomLevelF, ZOOM_LEVELS[0]), ZOOM_LEVELS[ZOOM_LEVELS.length - 1]);
		// Snap to the fixed level; no data-fitting cap -- zooming out leaves
		// leading empty space, as in any editor.
		let best = ZOOM_LEVELS[0];
		for (const l of ZOOM_LEVELS) {
			if (Math.abs(l - zoomLevelF) < Math.abs(best - zoomLevelF)) best = l;
		}
		zoomLevel = best;
		segsPerCol = Math.max(1, Math.round((DEFAULT_SEGS * sampleRate) / REFERENCE_RATE / zoomLevel));
		ensureCap();
		updateZoomLabel();
	}

	// A rate change rescales the time-normalized zoom.
	function setSampleRate(sr: number | undefined) {
		if (!sr || sr === sampleRate) return;
		sampleRate = sr;
		clampSegs();
		markDirty();
	}

	function clampView() {
		const availStart = dataStart();
		const plotW = Math.max(1, W - SCALE_W);
		// Capped at totalSegs: a wider view keeps the live edge rather than
		// pushing past it (which would flicker against `following`).
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
			// Zoom stays pinned to the live edge.
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
		const trackX = 4;
		const trackW = Math.max(1, W - 8);
		const thumbW = 24;
		const scrollable = Math.max(1, trackW - thumbW);
		const minViewEnd = availStart + visibleSegs;
		const denom = Math.max(0, totalSegs - minViewEnd);
		const posFrac = denom > 0 ? Math.min(1, Math.max(0, (viewEndSeg - minViewEnd) / denom)) : 0;
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

	function scrollbarPanByThumbPx(dx: number) {
		const m = scrollbarMetrics();
		if (m.scrollable <= 0 || m.denom <= 0) return;
		viewEndSeg += (dx / m.scrollable) * m.denom;
		clampView();
		following = viewEndSeg >= totalSegs;
		if (dx > 0) lastPanDir = -1;
		else if (dx < 0) lastPanDir = 1;
		markDirty();
	}

	function scrollbarJumpToPx(canvasX: number) {
		const m = scrollbarMetrics();
		if (m.scrollable <= 0 || m.denom <= 0) return;
		const trackRelX = canvasX - m.trackX;
		const targetThumbStart = trackRelX - m.thumbW / 2;
		const frac = Math.min(1, Math.max(0, targetThumbStart / m.scrollable));
		const minViewEnd = m.availStart + m.visibleSegs;
		viewEndSeg = minViewEnd + frac * m.denom;
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
		// Right-anchored grid: columns are multiples of `segsPerCol` from the
		// view end, so the rightmost column always covers the newest audio (a
		// left-anchored grid leaves a flat notch at the live edge).
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
				} else {
					// Out of the data range (before its start / past the edge):
					// nothing to load, so a stale "loading" flag must not stick.
					missing[k] = 0;
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
		// Floor, not ceil: a label straddling the left edge keeps rendering,
		// sliced by the draw clip. Clamped at 0 -- time starts at the take's
		// first frame, and negative seconds format as bogus large labels.
		const firstSample = Math.max(0, Math.floor((viewStartSeg * SEG_FRAMES) / stepSamples) * stepSamples);
		const outTicks: { x: number; label: string }[] = [];
		for (let s = firstSample; s < (viewStartSeg + plotW * segsPerCol) * SEG_FRAMES; s += stepSamples) {
			outTicks.push({
				// Fractional x so ticks glide with the stream.
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

	// Keeps every lane >= MIN_LANE_H; `fill` mode and the base height are the
	// floor.
	function applyMinHeight() {
		if (fill) return;
		const sbH = canScroll() ? SCROLL_H : 0;
		const need = TIME_H + channels * MIN_LANE_H + sbH;
		const next = Math.max(height, need);
		if (next !== H) H = next;
	}

	function laneMetrics() {
		const sbH = canScroll() ? SCROLL_H : 0;
		const laneH = (H - TIME_H - sbH) / channels;
		const halfH = Math.max(3, laneH / 2 - Math.min(VERT_PAD, laneH * 0.25));
		return { laneH, halfH };
	}

	function draw() {
		const c = ctx;
		// canvas/ctx are nulled on unmount; a rAF may still land after teardown.
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
					// Float recordings can exceed 1.0; clip to the lane.
					c.save();
					c.beginPath();
					c.rect(SCALE_W, top, W - SCALE_W, laneH);
					c.clip();
					c.beginPath();
					// Right-anchored: k runs right-to-left from the newest column;
					// +0.5 keeps the plot covered at every sub-pixel off.
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
					// Shade columns whose disk bins haven't arrived yet.
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
			c.moveTo(0, TIME_H - 1);
			c.lineTo(W, TIME_H - 1);
			c.stroke();

			if (!fileMode) {
				const availStart = dataStart();
				const plotW = Math.max(1, W - SCALE_W);
				const viewStartSeg = viewEndSeg - plotW * segsPerCol;
				const boundX = SCALE_W + (availStart - viewStartSeg) / segsPerCol;

				// Boundary line is visible on screen
				if (boundX >= SCALE_W && boundX <= W) {
					c.save();
					c.strokeStyle = 'rgba(239, 68, 68, 0.4)';
					c.lineWidth = 1;
					c.setLineDash([3, 3]);
					c.beginPath();
					c.moveTo(boundX, TIME_H);
					c.lineTo(boundX, H);
					c.stroke();
					c.restore();

					if (totalSegs > 0) {
						c.save();
						c.font = '6.5px monospace';
						const line1 = 'Live view · Buffer limit';
						const line2 = 'Earlier audio not cached on disk';
						const w1 = c.measureText(line1).width;
						const w2 = c.measureText(line2).width;
						const bw = Math.max(w1, w2) + 12;
						const bh = 22;
						const by = TIME_H + (H - TIME_H - bh) / 2;
						let bx = boundX + 4;
						if (bx + bw > W - 4) bx = boundX - bw - 4;
						bx = Math.max(SCALE_W + 4, bx);

						c.fillStyle = 'rgba(0, 0, 0, 0.8)';
						roundRect(c, bx, by, bw, bh, 3);
						c.fill();

						c.fillStyle = 'rgba(255, 255, 255, 0.8)';
						c.textBaseline = 'top';
						c.textAlign = 'left';
						c.fillText(line1, bx + 6, by + 3.5);
						c.fillStyle = 'rgba(255, 255, 255, 0.5)';
						c.fillText(line2, bx + 6, by + 12);
						c.restore();
					}
				}
			}

			// Clip to the plot area so boundary labels slice, not vanish.
			c.save();
			c.beginPath();
			c.rect(SCALE_W, 0, W - SCALE_W, H);
			c.clip();
			for (const t of ticks) {
				// Fade near the right edge so labels glide out, not clip.
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
				const sbTop = H - SCROLL_H;

				// Subtle divider separating waveform lanes from scrollbar area
				c.strokeStyle = 'rgba(255,255,255,0.12)';
				c.lineWidth = 1;
				c.beginPath();
				c.moveTo(0, sbTop);
				c.lineTo(W, sbTop);
				c.stroke();

				// Soft track background
				const trackH = 4;
				const trackY = sbTop + (SCROLL_H - trackH) / 2;
				c.fillStyle = 'rgba(255,255,255,0.06)';
				roundRect(c, m.trackX, trackY, m.trackW, trackH, 2);
				c.fill();

				// Matching rounded thumb; highlights on hover or drag
				const thumbH = 6;
				const thumbY = sbTop + (SCROLL_H - thumbH) / 2;
				c.fillStyle = scrollbarDragging || scrollbarHover ? 'rgba(255,255,255,0.6)' : 'rgba(255,255,255,0.35)';
				roundRect(c, m.thumbX, thumbY, m.thumbW, thumbH, 3);
				c.fill();
			}
		}
	}

	async function fetchPeaks(startSeg: number, count?: number) {
		if (!filePath || fetching || disposed) return;
		// Segments at/past the readable total cannot be cached yet (the read
		// comes back clamped and zeroed); the next flush re-arms the loader.
		if (fileLoaded && startSeg >= readableTotalSegs) return;
		fetching = true;
		const reqPath = filePath;
		const reqSession = session;
		try {
			const plotW = Math.max(1, W - SCALE_W);
			let want = Math.min(count ?? (fileLoaded ? Math.max(64, Math.ceil(plotW * segsPerCol) + 32) : 64), MAX_FETCH_SEGS);
			if (fileLoaded) {
				// Tail clamp: keep the read full-width near the readable end so it
				// still covers the caller's missing segment.
				if (startSeg + want > readableTotalSegs) {
					startSeg = Math.max(0, readableTotalSegs - want);
				}
			}
			const cnt = fileLoaded ? Math.min(want, readableTotalSegs - startSeg) : want;
			const res = await methods.readFilePeaks(reqPath, startSeg * SEG_FRAMES, SEG_FRAMES, cnt);
			// A read that raced a session switch or path change describes
			// replaced content; the never-regress totals would pin it forever.
			if (disposed || reqPath !== filePath || reqSession !== session) return;
			if (res.channels > 0) {
				fileChannels = res.channels;
				channels = maxChannels ? Math.min(res.channels, maxChannels) : res.channels;
				setSampleRate(res.sampleRate);
				applyMinHeight();
			}
			// Never regress: progress may already know a larger total than the
			// last flush. A zero read is "not loaded yet" -- keeping fileLoaded
			// false retries until the first flush lands.
			if (res.totalFrames > 0) {
				fileTotalSegs = Math.max(fileTotalSegs, Math.ceil(res.totalFrames / SEG_FRAMES));
				readableTotalSegs = Math.max(readableTotalSegs, Math.ceil(res.totalFrames / SEG_FRAMES));
				fileLoaded = true;
			}
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
			// While following, the scope stream owns the view end; the lagging
			// disk total must not step it.
			if (!(liveActive && following)) {
				totalSegs = fileTotalSegs;
				if (following) viewEndSeg = fileTotalSegs;
			}
			clampSegs();
			clampView();
			markDirty();
		} catch (e) {
			// Unreadable / not-yet-created file: latch off; onProgress re-arms
			// once progress proves content exists on disk.
			fileLoaded = true;
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
	// `fetching` guard paces it). Chunks are picked in pan-direction order, so
	// a pan lands on cached history instead of filling under the cursor. The
	// clean-scan key skips the O(span) rescan while idle; the end-probe below
	// is what re-arms loading when a fresh flush lands past the readable end.
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
		// Scan caps at the readable total, not the progress total: segments the
		// flush hasn't reached cannot be cached (the read comes back zeroed),
		// so scanning them would only rescan the missing tail every frame.
		const viewEnd = Math.min(Math.ceil(viewEndSeg), readableTotalSegs);
		const margin = spanSegs * PREFETCH_PLOTS;
		const lo = Math.max(0, viewStart - margin);
		const hi = Math.min(readableTotalSegs, viewEnd + margin);
		// End-probe: the only read that notices a flush past the readable
		// total, so it must run regardless of the scan window or key state.
		const probe = readableTotalSegs > 0 && readableTotalSegs < fileTotalSegs;
		if (lo >= hi) {
			if (probe) fetchPeaks(readableTotalSegs - 1, 1);
			return;
		}
		const key = `${lo}|${hi}`;
		if (key === scanCleanKey) {
			if (probe) fetchPeaks(readableTotalSegs - 1, 1);
			return;
		}
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
		if (probe) fetchPeaks(readableTotalSegs - 1, 1);
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
		const scaleX = rect.width > 0 ? W / rect.width : 1;
		const x = (e.clientX - rect.left) * scaleX;
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
		const scaleX = rect.width > 0 ? W / rect.width : 1;
		const scaleY = rect.height > 0 ? H / rect.height : 1;
		const x = (e.clientX - rect.left) * scaleX;
		const y = (e.clientY - rect.top) * scaleY;

		if (canScroll() && y >= H - SCROLL_H && y <= H) {
			const m = scrollbarMetrics();
			// Click anywhere in the bottom scrollbar block
			if (x >= m.trackX && x <= m.trackX + m.trackW) {
				if (x < m.thumbX || x > m.thumbX + m.thumbW) {
					scrollbarJumpToPx(x);
				}
				scrollbarDragging = true;
				lastX = e.clientX;
				e.preventDefault();
				return;
			}
		}
		dragging = true;
		lastX = e.clientX;
		e.preventDefault();
	}

	function onMove(e: MouseEvent) {
		const rect = wrap?.getBoundingClientRect();
		const scaleX = rect && rect.width > 0 ? W / rect.width : 1;
		const scaleY = rect && rect.height > 0 ? H / rect.height : 1;

		if (rect) {
			const x = (e.clientX - rect.left) * scaleX;
			const y = (e.clientY - rect.top) * scaleY;
			const m = scrollbarMetrics();
			scrollbarHover = canScroll() && y >= H - SCROLL_H && y <= H && x >= m.trackX && x <= m.trackX + m.trackW;
		}

		if (scrollbarDragging) {
			const dx = (e.clientX - lastX) * scaleX;
			lastX = e.clientX;
			scrollbarPanByThumbPx(dx);
			return;
		}
		if (!dragging) return;
		const dx = (e.clientX - lastX) * scaleX;
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
	// Set on destroy: mount-time listeners resolve asynchronously, and a fast
	// teardown would leave them waking a dead component.
	let disposed = false;

	// Reset and load from disk when a PCM-capable path is set or changes.
	$effect(() => {
		const p = filePath;
		const wantDisk = pcm === null ? isPcm(p) : pcm && p != null;
		if (!wantDisk) return;
		fileCache.clear();
		scanCleanKey = '';
		fileLoaded = false;
		fileTotalSegs = 0;
		readableTotalSegs = 0;
		fileChannels = 0;
		totalSegs = 0;
		viewEndSeg = 0;
		following = true;
		liveTotalSegs = 0;
		liveSessionFrames = 0;
		liveOpenAbsSeg = -1;
		liveFirstSeg = -1;
		ringFrom = -1;
		liveLastEnd = -1;
		liveBaseSeg = -1;
		liveActive = false;
		session = -1;
		markDirty();
	});

	// File mode: progress carries the real-time total; `stopped` hands the
	// tail back to disk for the final state.
	function onProgress(p: RecorderProgress) {
		if (p.nodeId !== nodeId || !fileMode) return;
		setSampleRate(p.sampleRate);
		const sid = p.session ?? 0;
		// Same forward-only rule as `onScope`.
		if (sid < session) return;
		if (sid !== session) adoptSession(sid, p.baseFrames ?? 0);
		if (p.frames > 0) {
			const segs = Math.max(1, Math.ceil(p.frames / SEG_FRAMES));
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
		}
		// Re-arm a read-error latch (fetchPeaks catch) once progress proves
		// content exists on disk.
		if (fileLoaded && readableTotalSegs === 0 && fileTotalSegs > 0) fileLoaded = false;
		if (p.stopped) {
			afterStop = true;
			liveActive = false;
			liveTotalSegs = 0;
			liveSessionFrames = 0;
			liveOpenAbsSeg = -1;
			liveFirstSeg = -1;
			liveLastEnd = -1;
			liveBaseSeg = 0;
			// Blank the view during a restart gap; with no fresh session the
			// timer below restores the recorded file.
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
	class:cursor-pointer={scrollbarHover && !scrollbarDragging}
	class:cursor-grabbing={dragging || scrollbarDragging}
	class:cursor-grab={pan && !dragging && !scrollbarDragging && !scrollbarHover}
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
		class="nodrag nopan absolute top-1.5 right-1.5 flex items-center gap-1"
		onmousedown={(e) => e.stopPropagation()}
		onwheel={(e) => e.stopPropagation()}
		ondblclick={(e) => e.stopPropagation()}>
		{#if pan && !following}
			<button
				type="button"
				class="flex h-4.5 items-center justify-center gap-0.5 rounded-md bg-neutral-900/40 px-1 text-white/70 backdrop-blur-[2px] hover:bg-neutral-900/60 hover:text-white"
				onclick={jumpToEnd}
				transition:blur={{ amount: 1 }}
				title="Jump to live edge">
				<ChevronDoubleRight class="size-2.5" />
			</button>
		{/if}
		<div class="flex h-4.5 items-center gap-0 rounded-md bg-neutral-900/40 px-0.5 backdrop-blur-[2px]">
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
	</div>
</div>
