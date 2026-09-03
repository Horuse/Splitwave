<script lang="ts">
	import { open, save } from '@tauri-apps/plugin-dialog';
	import { revealItemInDir } from '@tauri-apps/plugin-opener';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, untrack } from 'svelte';
	import { tauriListen } from '$lib/utils/tauri_event';
	import { useNodeConnections, useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type {
		AiffBitDepth,
		FileRecordingNodeData,
		FlacBitDepth,
		FlacCompression,
		OpusApplication,
		RecordingFormat,
		RecordingMode,
		WavBitDepth
	} from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { appSettings } from '$lib/modules/settings/stores.svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import Wrapper from '../node.svelte';
	import { Eye, EyeOff, Folder, FolderOpen, FileRecord, Pulse } from '$lib/components/icons';
	import { RECORDING_FORMATS } from '$lib/modules/pipeline/recording-formats';
	import NumberStepper from '$lib/components/number_stepper.svelte';
	import { onNodeAction, parseHandle } from '$lib/modules/flow/utils';
	import SegmentedButtons from '$lib/components/segmented_buttons.svelte';
	import WaveformScope from '$lib/components/waveform_scope.svelte';
	import { Tooltip } from '$lib/modules/overlay/ui';
	import { modalManager } from '$lib/modules/overlay/modal';
	import { ConfirmModal } from '$lib/modules/overlay/ui';
	import { platform } from '@tauri-apps/plugin-os';
	import { getContext } from 'svelte';
	import { PREVIEW_CTX } from '$lib/modules/flow/utils';

	const isPreview = getContext(PREVIEW_CTX) === true;
	const isMac = isPreview || platform() === 'macos';
	const isWindows = platform() === 'windows';

	type FileRecordingNodeType = Node<FileRecordingNodeData, 'fileRecording'>;
	let { id, data }: NodeProps<FileRecordingNodeType> = $props();

	const flow = useSvelteFlow();

	interface ProgressEvent {
		nodeId: string;
		frames: number;
		sampleRate: number;
		stopped?: boolean;
	}

	let frames = $state(0);
	let sampleRate = $state(0);
	let recording = $state(false);
	let committedFormat = $state<RecordingFormat | null>(null);
	let committedMode = $state<RecordingMode | null>(null);

	let unlistenChoose: (() => void) | undefined;
	unlistenChoose = onNodeAction(id, 'chooseFile', () => {
		chooseFile().catch(() => {});
	});
	tauriListen<ProgressEvent>('audio://recorder_progress', (p) => {
		if (p.nodeId !== id) return;
		frames = p.frames;
		sampleRate = p.sampleRate;
		if (!p.stopped) {
			recording = true;
		} else {
			recording = false;
			committedFormat = null;
		}
	});

	$effect(() => {
		if (audioStore.isRunning) {
			if (untrack(() => committedFormat) === null) {
				committedFormat = untrack(() => data.format);
				committedMode = untrack(() => data.mode);
			}
		} else {
			const cf = untrack(() => committedFormat);
			const fmt = untrack(() => data.format);
			const fp = untrack(() => data.filePath);
			if (cf !== null && cf.kind !== fmt.kind && fp) {
				flow.updateNodeData(id, { filePath: replaceExtension(fp, extension(fmt)) });
			}
			recording = false;
			committedFormat = null;
			committedMode = null;
		}
	});

	$effect(() => {
		if (audioStore.chooseFileNodeId !== id) return;
		audioStore.chooseFileNodeId = null;
		const retryId = audioStore.pendingRetryPipelineId;
		audioStore.pendingRetryPipelineId = null;
		chooseFile()
			.then(async (picked) => {
				if (!picked || retryId === null) {
					// Cancelled after a failed start in New mode: the node is left
					// pointing at an existing file it cannot write to, so offer
					// the explicit overwrite confirmation as the way out.
					if (!picked && retryId !== null && mode === 'new' && data.filePath) {
						await confirmModeSwitch('overwrite');
					}
					return;
				}
				const snapshot = pipelineStore.editorActions?.getSnapshot();
				if (!snapshot) return;
				// Keep the node's mode in sync with the activated snapshot, or
				// the next manual activation would fail on the existing file.
				flow.updateNodeData(id, { mode: 'overwrite' });
				const nodes = snapshot.nodes.map((n) => (n.id === id ? { ...n, data: { ...n.data, mode: 'overwrite' } } : n));
				audioStore.activatePipeline(retryId, { nodes, edges: snapshot.edges }).catch((e) => audioStore.reportError(e));
			})
			.catch(() => {});
	});

	onDestroy(() => {
		unlistenChoose?.();
	});

	// Per-format capability config (extension, channel cap, rate grid, bitrate
	// presets/bounds) lives in `recording-formats.ts`, same module as the
	// graph types.
	let cfg = $derived(RECORDING_FORMATS[data.format.kind]);

	function extension(fmt: RecordingFormat): string {
		return RECORDING_FORMATS[fmt.kind].extension;
	}

	function isAppendable(fmt: RecordingFormat): boolean {
		return fmt.kind === 'wav' || fmt.kind === 'aiff';
	}

	const ALL_FORMATS = [
		{ value: 'wav' as const, label: 'WAV' },
		{ value: 'flac' as const, label: 'FLAC' },
		{ value: 'aiff' as const, label: 'AIFF' },
		{ value: 'opus' as const, label: 'Opus' },
		{ value: 'mp3' as const, label: 'MP3' },
		{ value: 'aac' as const, label: 'AAC' }
	];

	const FORMATS = isMac ? ALL_FORMATS : ALL_FORMATS.filter((f) => f.value !== 'aac');

	$effect(() => {
		if (!isMac && data.format.kind === 'aac') {
			untrack(() => {
				flow.updateNodeData(id, {
					format: { kind: 'wav', bitDepth: 'f32' },
					...(data.filePath ? { filePath: replaceExtension(data.filePath, 'wav') } : {})
				});
			});
		}
	});

	const MODES = [
		{ value: 'new' as const, label: 'New' },
		{ value: 'overwrite' as const, label: 'Overwrite' },
		{ value: 'append' as const, label: 'Append' }
	];

	let appendable = $derived(isAppendable(data.format));
	let modeOptions = $derived(MODES.map((m) => (m.value === 'append' ? { ...m, disabled: !appendable } : m)));
	let mode = $derived<RecordingMode>(data.mode ?? 'new');
	// Append extends an existing file byte-for-byte, so the encoder shape is
	// frozen: format, bit depth, bitrate, channels and sample rate are all
	// read-only until the mode changes.
	let locked = $derived(mode === 'append');

	let maxChannels = $derived(cfg.maxChannels);

	let CHANNEL_MODES = $derived([
		{ value: 'mono' as const, label: 'Mono', disabled: locked },
		{ value: 'stereo' as const, label: 'Stereo', disabled: locked },
		{ value: 'multi' as const, label: 'Multi', disabled: maxChannels <= 2 || locked }
	]);

	type ChannelMode = 'mono' | 'stereo' | 'multi';
	let channelMode = $derived<ChannelMode>(data.channels <= 1 ? 'mono' : data.channels === 2 ? 'stereo' : 'multi');

	let channelLabel = $derived(data.channels <= 1 ? 'mono' : data.channels === 2 ? 'stereo' : `${data.channels} ch`);

	async function setChannelMode(m: ChannelMode) {
		if (!(await confirmOverwriteChange('changing the channel layout'))) return;
		const target = m === 'mono' ? 1 : m === 'stereo' ? 2 : Math.max(3, data.channels);
		const channels = Math.min(target, maxChannels);
		dropEdgesAbove(channels);
		// Fewer channels narrow the per-channel bitrate cap (AAC).
		const patch: Partial<FileRecordingNodeData> = { channels };
		const fmt = clampFormatBitrate(data.format, data.sampleRate ?? 48_000, channels);
		if (fmt !== data.format) patch.format = fmt;
		flow.updateNodeData(id, patch);
	}

	function dropEdgesAbove(cap: number) {
		const orphaned = flow
			.getEdges()
			.filter((e) => {
				if (e.target !== id) return false;
				const ch = e.targetHandle ? parseHandle(e.targetHandle) : null;
				return ch !== null && ch > cap;
			})
			.map((e) => ({ id: e.id }));
		if (orphaned.length > 0) flow.deleteElements({ edges: orphaned });
	}
	const wired = useNodeConnections({ id, handleType: 'target' });
	let wiredChannels = $derived(
		wired.current.reduce((n, c) => {
			const ch = c.targetHandle ? parseHandle(c.targetHandle) : null;
			return ch === null ? n : Math.max(n, ch);
		}, 0)
	);

	// Mono and stereo pin the encoder width; multi lets the cables drive it.
	let slotCap = $derived(channelMode === 'mono' ? 1 : channelMode === 'stereo' ? 2 : maxChannels);

	$effect(() => {
		if (channelMode !== 'multi') return;
		const next = Math.max(3, Math.min(wiredChannels, maxChannels));
		if (next !== untrack(() => data.channels)) flow.updateNodeData(id, { channels: next });
	});

	// A narrower format strands cables it cannot carry.
	$effect(() => {
		const cap = slotCap;
		untrack(() => {
			dropEdgesAbove(cap);
			if (data.channels > cap) flow.updateNodeData(id, { channels: cap });
		});
	});

	async function chooseFile(): Promise<boolean> {
		const ext = extension(data.format);
		if (mode === 'append') {
			const path = await open({
				title: 'Choose recording to append to',
				multiple: false,
				filters: [{ name: ext.toUpperCase(), extensions: [ext] }]
			});
			if (typeof path !== 'string') return false;
			flow.updateNodeData(id, { filePath: path });
			return true;
		}
		const path = await save({
			title: 'Save recording',
			filters: [{ name: ext.toUpperCase(), extensions: [ext] }]
		});
		if (!path) return false;
		flow.updateNodeData(id, { filePath: path });
		return true;
	}

	function replaceExtension(path: string, newExt: string): string {
		const lastSlash = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
		const lastDot = path.lastIndexOf('.');
		if (lastDot > lastSlash) {
			return `${path.slice(0, lastDot + 1)}${newExt}`;
		}
		return `${path}.${newExt}`;
	}

	async function setFormatKind(kind: 'wav' | 'flac' | 'opus' | 'mp3' | 'aac' | 'aiff') {
		if (!(await confirmOverwriteChange('changing the format'))) return;
		if (data.format.kind === kind) return;
		let next: RecordingFormat;
		if (kind === 'wav') next = { kind: 'wav', bitDepth: 'f32' };
		else if (kind === 'flac') next = { kind: 'flac', bitDepth: 'i24', compression: 'default' };
		else if (kind === 'opus') next = { kind: 'opus', bitrate: 128_000, application: 'audio' };
		else if (kind === 'mp3') next = { kind: 'mp3', bitrateKbps: 192 };
		else if (kind === 'aac') next = { kind: 'aac', bitrate: 192_000 };
		else next = { kind: 'aiff', bitDepth: 'i24' };
		const cfgNext = RECORDING_FORMATS[kind];
		// The carried-over shape must fit the new encoder: grid formats snap
		// the rate to the nearest supported value, custom ranges clamp, and
		// the default bitrate is re-clamped to the bounds at that shape.
		let rate = data.sampleRate ?? 48_000;
		if (cfgNext.rate.mode === 'grid' && cfgNext.rate.rates) {
			rate = cfgNext.rate.rates.reduce((best, r) => (Math.abs(r - rate) < Math.abs(best - rate) ? r : best));
		} else if (cfgNext.rate.mode === 'grid+custom') {
			rate = Math.min(cfgNext.rate.max ?? 384_000, Math.max(cfgNext.rate.min ?? 8_000, rate));
		}
		next = clampFormatBitrate(next, rate, Math.min(data.channels, cfgNext.maxChannels));
		const patch: Partial<FileRecordingNodeData> = { format: next };
		if (cfgNext.rate.mode !== 'fixed') patch.sampleRate = rate;
		if (mode === 'append' && !isAppendable(next)) {
			patch.mode = 'new';
		}
		// The path names the *next* recording: a pending change keeps the
		// running recorder on the activated path (it shows "changes pending"),
		// while the node already points at the renamed target.
		if (data.filePath) {
			patch.filePath = replaceExtension(data.filePath, extension(next));
		}
		flow.updateNodeData(id, patch);
	}

	function setMode(m: RecordingMode) {
		if (mode === m) return;
		confirmModeSwitch(m).catch(() => {});
	}

	// Overwrite rewrites the file from scratch on the next recording, so any
	// change to the encoder shape while it is armed must be confirmed. The
	// confirmation can be skipped per file (modal checkbox) or disabled
	// entirely in Settings.
	const OVERWRITE_SKIP_KEY = 'recording:overwriteSkip';
	let overwriteSkip: Set<string> = loadOverwriteSkip();
	function loadOverwriteSkip(): Set<string> {
		if (typeof window === 'undefined') return new Set();
		try {
			return new Set(JSON.parse(window.localStorage.getItem(OVERWRITE_SKIP_KEY) ?? '[]'));
		} catch {
			return new Set();
		}
	}

	async function confirmOverwriteChange(what: string): Promise<boolean> {
		if (!appSettings.confirmOverwriteChanges) return true;
		const path = data.filePath;
		if (mode !== 'overwrite' || !path) return true;
		if (overwriteSkip.has(path)) return true;
		const res = await modalManager.open<boolean | { ok: boolean; dontAskAgain: boolean }>('Recording will be erased', ConfirmModal, {
			message: `In Overwrite mode, ${what} erases "${basename(path)}" the next time you record.`,
			confirmLabel: 'Change anyway',
			danger: true,
			checkboxLabel: "Don't ask again for this file",
			warning: audioStore.isRunning
				? 'The pipeline is running right now — confirming restarts this recording immediately and the existing file is erased at once.'
				: undefined
		});
		if (typeof res === 'object') {
			if (res.ok && res.dontAskAgain) {
				overwriteSkip.add(path);
				window.localStorage.setItem(OVERWRITE_SKIP_KEY, JSON.stringify([...overwriteSkip]));
			}
			return res.ok;
		}
		return res === true;
	}

	// Append extends the file in place; switching that node to Overwrite makes
	// the next recording erase everything recorded so far, so require an
	// explicit confirmation before the mode changes. Other transitions are
	// safe: New just fails to start on an existing file, and choosing an
	// existing path in Overwrite is confirmed by the native save dialog.
	async function confirmModeSwitch(m: RecordingMode): Promise<void> {
		if (mode === 'append' && m === 'overwrite' && data.filePath) {
			const ok = await confirmOverwriteChange('switching to Overwrite');
			if (!ok) return;
		}
		flow.updateNodeData(id, { mode: m });
	}

	async function setWavBitDepth(bd: WavBitDepth) {
		if (!(await confirmOverwriteChange('changing the bit depth'))) return;
		if (data.format.kind !== 'wav') return;
		flow.updateNodeData(id, { format: { kind: 'wav', bitDepth: bd } });
	}

	async function setFlacBitDepth(bd: FlacBitDepth) {
		if (!(await confirmOverwriteChange('changing the bit depth'))) return;
		if (data.format.kind !== 'flac') return;
		flow.updateNodeData(id, {
			format: { kind: 'flac', bitDepth: bd, compression: data.format.compression }
		});
	}

	async function setFlacCompression(c: FlacCompression) {
		if (!(await confirmOverwriteChange('changing the compression'))) return;
		if (data.format.kind !== 'flac') return;
		flow.updateNodeData(id, {
			format: { kind: 'flac', bitDepth: data.format.bitDepth, compression: c }
		});
	}

	async function setOpusBitrate(bps: number | string) {
		if (typeof bps === 'string') {
			customBitrateSelected = true;
			return;
		}
		if (!(await confirmOverwriteChange('changing the bitrate'))) return;
		if (data.format.kind !== 'opus') return;
		customBitrateSelected = false;
		flow.updateNodeData(id, {
			format: { kind: 'opus', bitrate: bps, application: data.format.application }
		});
	}

	async function setOpusApplication(a: OpusApplication) {
		if (!(await confirmOverwriteChange('changing the application mode'))) return;
		if (data.format.kind !== 'opus') return;
		flow.updateNodeData(id, {
			format: { kind: 'opus', bitrate: data.format.bitrate, application: a }
		});
	}

	async function setMp3Bitrate(kbps: number | string) {
		if (typeof kbps === 'string') {
			customBitrateSelected = true;
			return;
		}
		if (!(await confirmOverwriteChange('changing the bitrate'))) return;
		if (data.format.kind !== 'mp3') return;
		customBitrateSelected = false;
		flow.updateNodeData(id, { format: { ...data.format, bitrateKbps: kbps } });
	}

	async function setAacBitrate(bps: number | string) {
		if (typeof bps === 'string') {
			customBitrateSelected = true;
			return;
		}
		if (!(await confirmOverwriteChange('changing the bitrate'))) return;
		if (data.format.kind !== 'aac') return;
		customBitrateSelected = false;
		flow.updateNodeData(id, { format: { ...data.format, bitrate: bps } });
	}

	async function setAiffBitDepth(bd: AiffBitDepth) {
		if (!(await confirmOverwriteChange('changing the bit depth'))) return;
		if (data.format.kind !== 'aiff') return;
		flow.updateNodeData(id, { format: { kind: 'aiff', bitDepth: bd } });
	}

	async function revealFolder() {
		if (!data.filePath) return;
		await revealItemInDir(data.filePath);
	}

	function basename(p: string | null): string {
		if (!p) return 'No file selected';
		const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
		return idx >= 0 ? p.slice(idx + 1) : p;
	}

	function formatDuration(sec: number): string {
		const minutes = Math.floor(sec / 60);
		const remainder = sec - minutes * 60;
		return `${minutes}:${remainder.toFixed(1).padStart(4, '0')}`;
	}

	function formatSize(bytes: number): string {
		if (bytes < 1024) return `${bytes} B`;
		if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
		if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
		return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
	}

	const WAV_BIT_DEPTHS: { value: WavBitDepth; label: string; sub: string }[] = [
		{ value: 'i16', label: '16-bit', sub: 'PCM' },
		{ value: 'i24', label: '24-bit', sub: 'PCM' },
		{ value: 'f32', label: '32-bit', sub: 'float' }
	];

	const FLAC_BIT_DEPTHS: { value: FlacBitDepth; label: string }[] = [
		{ value: 'i16', label: '16-bit' },
		{ value: 'i24', label: '24-bit' }
	];

	const FLAC_COMPRESSIONS: { value: FlacCompression; label: string }[] = [
		{ value: 'fast', label: 'Fast' },
		{ value: 'default', label: 'Default' },
		{ value: 'best', label: 'Best' }
	];

	const OPUS_APPLICATIONS: { value: OpusApplication; label: string; sub: string }[] = [
		{ value: 'audio', label: 'Audio', sub: 'music' },
		{ value: 'voip', label: 'VoIP', sub: 'voice' },
		{ value: 'low-delay', label: 'Low', sub: 'delay' }
	];

	const AIFF_BIT_DEPTHS: { value: AiffBitDepth; label: string }[] = [
		{ value: 'i16', label: '16-bit' },
		{ value: 'i24', label: '24-bit' }
	];

	function kHz(n: number): string {
		const k = n / 1000;
		return String(Number.isInteger(k) ? k : Number(k.toFixed(3)));
	}

	// Custom is a UI choice that only reveals the numeric input, so it cannot
	// be derived from `sampleRate` alone.
	let customRateSelected = $state(false);
	let customBitrateSelected = $state(false);

	let rateValues = $derived(new Set((cfg.rate.rates ?? []).map(String)));
	let rateSelection = $derived(customRateSelected || !rateValues.has(String(data.sampleRate ?? 0)) ? 'custom' : String(data.sampleRate));
	let rateOptions = $derived(
		(cfg.rate.rates ?? [])
			.map((r) => ({ value: String(r), label: kHz(r) }))
			.concat(cfg.rate.mode === 'grid+custom' ? [{ value: 'custom', label: 'Custom' }] : [])
			.map((r) => ({ ...r, disabled: locked }))
	);

	// Bitrate bounds in kbps for a format at the given rate/channel shape.
	function bitrateBoundsFor(kind: RecordingFormat['kind'], rate: number, channels: number): [number, number] {
		const b = RECORDING_FORMATS[kind].bitrate;
		if (!b) return [0, 0];
		const base = b.boundsByRate[String(rate)] ?? b.boundsByRate.default;
		if (!b.perChannel) return [base.min, base.max];
		return [base.min * channels, Math.min(base.max * channels, b.absoluteMax ?? Number.MAX_SAFE_INTEGER)];
	}

	function bitrateBounds(): [number, number] {
		return bitrateBoundsFor(data.format.kind, data.sampleRate ?? 0, data.channels);
	}

	// Re-wraps the format with its bitrate clamped into the encoder bounds at
	// the given rate/channel shape (mp3 stores kbps, aac/opus store bps).
	function clampFormatBitrate(fmt: RecordingFormat, rate: number, channels: number): RecordingFormat {
		if (fmt.kind !== 'mp3' && fmt.kind !== 'aac' && fmt.kind !== 'opus') return fmt;
		const [min, max] = bitrateBoundsFor(fmt.kind, rate, channels);
		const clamp = (n: number) => Math.min(max, Math.max(min, n));
		if (fmt.kind === 'mp3') return { ...fmt, bitrateKbps: clamp(fmt.bitrateKbps) };
		return { ...fmt, bitrate: clamp(Math.round(fmt.bitrate / 1000)) * 1000 };
	}

	// Popular values first: the grid shows the curated presets within the
	// current bounds, so the common cases never need Custom. Thin coverage
	// falls back to an even ladder sampling.
	function bitratePresets(): number[] {
		const b = cfg.bitrate;
		if (!b) return [];
		const [min, max] = bitrateBounds();
		const inBounds = (list: number[]) => list.filter((k) => k >= min && k <= max);
		const preferred = inBounds(b.presets);
		if (preferred.length >= 3) return preferred.slice(-6);
		const ladder = inBounds(b.ladder);
		if (ladder.length <= 5) return ladder;
		const picked: number[] = [];
		for (let i = 0; i < 5; i++) {
			const v = ladder[Math.round((i * (ladder.length - 1)) / 4)];
			if (picked[picked.length - 1] !== v) picked.push(v);
		}
		return picked;
	}

	function bitrateOptions(): { value: number | string; label: string; disabled: boolean }[] {
		const b = cfg.bitrate;
		if (!b) return [];
		// mp3 stores kbps, aac/opus store bps.
		const opts: { value: number | string; label: string; disabled: boolean }[] = bitratePresets().map((k) => ({
			value: b.storedUnit === 'kbps' ? k : k * 1000,
			label: String(k),
			disabled: locked
		}));
		return opts.concat([{ value: 'custom', label: 'Custom', disabled: locked }]);
	}

	function bitrateIsPreset(): boolean {
		const f = data.format;
		const presets = bitratePresets();
		if (f.kind === 'mp3') return presets.some((k) => k === f.bitrateKbps);
		if (f.kind === 'aac' || f.kind === 'opus') return presets.some((k) => k * 1000 === f.bitrate);
		return false;
	}

	let showCustomBitrate = $derived(customBitrateSelected || !bitrateIsPreset());

	async function setCustomBitrate(kbps: number) {
		const b = cfg.bitrate;
		if (locked || !b) return;
		if (!(await confirmOverwriteChange('changing the bitrate'))) return;
		const [min, max] = bitrateBounds();
		const v = Math.min(max, Math.max(min, kbps));
		const value = b.storedUnit === 'kbps' ? v : v * 1000;
		if (data.format.kind === 'mp3') {
			flow.updateNodeData(id, { format: { ...data.format, bitrateKbps: value } });
		} else {
			flow.updateNodeData(id, { format: { ...data.format, bitrate: value } });
		}
	}

	async function setRateSelection(sel: string) {
		if (locked) return;
		if (sel === 'custom') {
			// No rate change yet -- the confirm belongs to the numeric input.
			customRateSelected = true;
			return;
		}
		if (!(await confirmOverwriteChange('changing the sample rate'))) return;
		customRateSelected = false;
		const rate = Number(sel);
		// A lower rate tier narrows the bitrate bounds (AAC at 32 kHz).
		const patch: Partial<FileRecordingNodeData> = { sampleRate: rate };
		const fmt = clampFormatBitrate(data.format, rate, data.channels);
		if (fmt !== data.format) patch.format = fmt;
		flow.updateNodeData(id, patch);
	}

	async function setCustomRate(n: number) {
		if (locked) return;
		if (!(await confirmOverwriteChange('changing the sample rate'))) return;
		flow.updateNodeData(id, { sampleRate: Math.min(cfg.rate.max ?? 384_000, Math.max(cfg.rate.min ?? 8_000, n)) });
	}

	const AIFF_BYTES_PER_FRAME: Record<AiffBitDepth, number> = { i16: 4, i24: 6 };

	const WAV_BYTES_PER_FRAME: Record<WavBitDepth, number> = { i16: 4, i24: 6, f32: 8 };
	const WAV_HEADER_BYTES: Record<WavBitDepth, number> = { i16: 44, i24: 44, f32: 58 };

	function estimatedSize(): number {
		const sr = sampleRate > 0 ? sampleRate : 48_000;
		const seconds = frames / sr;
		if (data.format.kind === 'wav') {
			return frames * WAV_BYTES_PER_FRAME[data.format.bitDepth] + WAV_HEADER_BYTES[data.format.bitDepth];
		}
		if (data.format.kind === 'flac') {
			const bpf = data.format.bitDepth === 'i16' ? 4 : 6;
			return Math.round(frames * bpf * 0.6 + 4096);
		}
		if (data.format.kind === 'opus') {
			return Math.round((data.format.bitrate / 8) * seconds * 1.05 + 4096);
		}
		if (data.format.kind === 'mp3') {
			return Math.round(((data.format.bitrateKbps * 1000) / 8) * seconds + 512);
		}
		if (data.format.kind === 'aac') {
			// AAC in M4A: ~3% MP4 container overhead.
			return Math.round((data.format.bitrate / 8) * seconds * 1.03 + 4096);
		}
		return frames * AIFF_BYTES_PER_FRAME[data.format.bitDepth] + 54;
	}

	function formatLabelFor(fmt: RecordingFormat): string {
		if (fmt.kind === 'wav') {
			const bd = fmt.bitDepth;
			return bd === 'i16' ? 'WAV PCM 16-bit' : bd === 'i24' ? 'WAV PCM 24-bit' : 'WAV 32-bit float';
		}
		if (fmt.kind === 'flac') {
			return `FLAC ${fmt.bitDepth === 'i24' ? '24-bit' : '16-bit'} · ${fmt.compression}`;
		}
		if (fmt.kind === 'opus') {
			return `Opus ${Math.round(fmt.bitrate / 1000)} kbps · ${fmt.application}`;
		}
		if (fmt.kind === 'mp3') {
			return `MP3 ${fmt.bitrateKbps} kbps`;
		}
		if (fmt.kind === 'aac') {
			return `AAC ${Math.round(fmt.bitrate / 1000)} kbps · M4A`;
		}
		return `AIFF PCM ${fmt.bitDepth === 'i24' ? '24-bit' : '16-bit'}`;
	}

	let estSize = $derived(estimatedSize());
	let durationSec = $derived(sampleRate > 0 ? frames / sampleRate : 0);
	let dirty = $derived(recording && committedFormat !== null && (JSON.stringify(committedFormat) !== JSON.stringify(data.format) || committedMode !== mode));
	let waveVisible = $derived(!(data.waveformHidden ?? false));
	// Waveform shows only lanes that have a cable; a phantom multi lane with no
	// handle stays hidden even though the encoder width may exceed it. With no
	// cables at all the cap lifts: an existing recording's own channel count
	// (from the file header) is then what the scope displays.
	let waveformChannels = $derived(channelMode === 'multi' ? (wiredChannels > 0 ? wiredChannels : null) : slotCap);

	function toggleWaveform() {
		flow.updateNodeData(id, { waveformHidden: !(data.waveformHidden ?? false) });
	}
</script>

<Wrapper label="File Recording" icon={FileRecord} accent="output" hasInput channelIo nodeId={id} maxChannels={slotCap}>
	<div class="flex w-64 flex-col gap-1.5">
		<div class="truncate rounded bg-neutral-100 px-2 py-1 text-xs text-neutral-1000" title={data.filePath ?? undefined}>
			{basename(data.filePath)}
		</div>
		<div class="flex gap-1">
			<button
				type="button"
				class="nodrag nopan button-main primary flex h-7 flex-1 items-center justify-center gap-1.5 rounded-lg py-0 text-xs"
				onclick={chooseFile}>
				<Folder class="size-3.5" />
				Choose file
			</button>
			<Tooltip
				text={isMac ? 'Reveal the recording in Finder' : isWindows ? 'Reveal the recording in File Explorer' : 'Reveal the recording in file manager'}>
				<button type="button" class="nodrag nopan button-main primary size-7 shrink-0 rounded-lg p-0" disabled={!data.filePath} onclick={revealFolder}>
					<FolderOpen class="size-3.5" />
				</button>
			</Tooltip>
		</div>

		<SegmentedButtons options={modeOptions} value={mode} onSelect={setMode} label="Mode" columns={3} />

		<SegmentedButtons options={FORMATS.map((f) => ({ ...f, disabled: locked }))} value={data.format.kind} onSelect={setFormatKind} columns={3} />

		<SegmentedButtons
			label="Channels"
			note={maxChannels >= 512 ? 'no limit' : `max ${maxChannels}`}
			options={CHANNEL_MODES}
			value={channelMode}
			onSelect={setChannelMode} />

		{#if cfg.rate.mode !== 'fixed'}
			<SegmentedButtons
				label="Sample rate"
				note="kHz"
				options={rateOptions}
				value={rateSelection}
				onSelect={setRateSelection}
				columns={cfg.rate.columns} />
			{#if rateSelection === 'custom' && cfg.rate.mode === 'grid+custom'}
				<div class="flex items-center justify-end gap-1">
					<span class="font-mono text-[9px] text-neutral-500">Hz</span>
					<NumberStepper
						value={data.sampleRate ?? 48_000}
						min={cfg.rate.min ?? 8000}
						max={cfg.rate.max ?? 384000}
						step={100}
						disabled={locked}
						label="Sample rate"
						onchange={setCustomRate} />
				</div>
			{/if}
		{/if}

		{#if data.format.kind === 'wav'}
			<SegmentedButtons
				options={WAV_BIT_DEPTHS.map((b) => ({ value: b.value, label: b.label, subtitle: b.sub, disabled: locked }))}
				value={data.format.bitDepth}
				onSelect={setWavBitDepth} />
		{:else if data.format.kind === 'flac'}
			<SegmentedButtons options={FLAC_BIT_DEPTHS.map((b) => ({ ...b, disabled: locked }))} value={data.format.bitDepth} onSelect={setFlacBitDepth} />
			<SegmentedButtons
				options={FLAC_COMPRESSIONS.map((c) => ({ ...c, disabled: locked }))}
				value={data.format.compression}
				onSelect={setFlacCompression} />
		{:else if data.format.kind === 'opus'}
			<SegmentedButtons
				label="Bitrate"
				note="kbps"
				options={bitrateOptions()}
				value={customBitrateSelected ? 'custom' : data.format.bitrate}
				onSelect={setOpusBitrate} />
			{#if showCustomBitrate}
				<div class="flex items-center justify-end gap-1">
					<span class="font-mono text-[9px] text-neutral-500">kbps</span>
					<NumberStepper
						value={Math.round(data.format.bitrate / 1000)}
						min={bitrateBounds()[0]}
						max={bitrateBounds()[1]}
						step={cfg.bitrate?.step}
						disabled={locked}
						label="Bitrate"
						onchange={setCustomBitrate} />
				</div>
			{/if}
			<SegmentedButtons
				options={OPUS_APPLICATIONS.map((a) => ({ value: a.value, label: a.label, subtitle: a.sub, disabled: locked }))}
				value={data.format.application}
				onSelect={setOpusApplication} />
		{:else if data.format.kind === 'mp3'}
			<SegmentedButtons
				label="Bitrate"
				note="kbps"
				options={bitrateOptions()}
				value={customBitrateSelected ? 'custom' : data.format.bitrateKbps}
				onSelect={setMp3Bitrate} />
			{#if showCustomBitrate}
				<div class="flex items-center justify-end gap-1">
					<span class="font-mono text-[9px] text-neutral-500">kbps</span>
					<NumberStepper
						value={data.format.bitrateKbps}
						min={bitrateBounds()[0]}
						max={bitrateBounds()[1]}
						step={cfg.bitrate?.step}
						disabled={locked}
						label="Bitrate"
						onchange={setCustomBitrate} />
				</div>
			{/if}
		{:else if data.format.kind === 'aac'}
			<SegmentedButtons
				label="Bitrate"
				note="kbps"
				options={bitrateOptions()}
				value={customBitrateSelected ? 'custom' : data.format.bitrate}
				onSelect={setAacBitrate} />
			{#if showCustomBitrate}
				<div class="flex items-center justify-end gap-1">
					<span class="font-mono text-[9px] text-neutral-500">kbps</span>
					<NumberStepper
						value={Math.round(data.format.bitrate / 1000)}
						min={bitrateBounds()[0]}
						max={bitrateBounds()[1]}
						step={cfg.bitrate?.step}
						disabled={locked}
						label="Bitrate"
						onchange={setCustomBitrate} />
				</div>
			{/if}
		{:else}
			<SegmentedButtons options={AIFF_BIT_DEPTHS.map((b) => ({ ...b, disabled: locked }))} value={data.format.bitDepth} onSelect={setAiffBitDepth} />
		{/if}

		<div class="flex items-baseline justify-between font-mono text-[11px]">
			<span class={recording ? 'text-red-500' : 'text-neutral-900'}>
				{recording ? '● REC' : '○'}
			</span>
			<span class="text-neutral-1000 tabular-nums">{formatDuration(durationSec)}</span>
		</div>
		<div class="flex justify-between text-[10px] text-neutral-900">
			<span class="truncate">
				{formatLabelFor(recording && committedFormat !== null ? committedFormat : data.format)}
				· {data.format.kind === 'opus' || data.format.kind === 'mp3' ? '48 kHz' : `${(data.sampleRate ?? 48_000) / 1000} kHz`}
				· {channelLabel}
			</span>
			<span class="font-mono tabular-nums">{formatSize(estSize)}</span>
		</div>
		{#if dirty}
			<div class="text-[9px] text-amber-600">changes pending - restart or choose new file</div>
		{/if}

		<div class="flex items-center justify-between border-t border-neutral-200 pt-1">
			<span class="flex items-center gap-1 font-mono text-[9px] text-neutral-500">
				<Pulse class="size-3" />
				Waveform
				{#if !isAppendable(data.format)}
					<!-- Non-PCM formats have no disk peak source: the scope shows
					only the live tail, no browsable history. -->
					<span class="text-neutral-400">· realtime, no history</span>
				{/if}
			</span>
			<Tooltip text={waveVisible ? 'Hide waveform' : 'Show waveform'}>
				<button type="button" class="nodrag nopan button-main primary size-4 p-0" onclick={toggleWaveform}>
					{#if waveVisible}
						<EyeOff class="size-2.5" />
					{:else}
						<Eye class="size-2.5" />
					{/if}
				</button>
			</Tooltip>
		</div>
		{#if waveVisible}
			<WaveformScope
				nodeId={id}
				filePath={data.filePath}
				pcm={data.format.kind === 'wav' || data.format.kind === 'aiff'}
				maxChannels={waveformChannels} />
		{/if}
	</div>
</Wrapper>
