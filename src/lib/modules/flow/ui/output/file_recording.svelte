<script lang="ts">
	import { save } from '@tauri-apps/plugin-dialog';
	import { revealItemInDir } from '@tauri-apps/plugin-opener';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount, untrack } from 'svelte';
	import { useNodeConnections, useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type {
		AiffBitDepth,
		FileRecordingNodeData,
		FlacBitDepth,
		FlacCompression,
		OpusApplication,
		RecordingFormat,
		WavBitDepth
	} from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import Wrapper from '../node.svelte';
	import { Folder, FolderOpen, FileRecord } from '$lib/components/icons';
	import { onNodeAction, parseHandle } from '$lib/modules/flow/utils';
	import SegmentedButtons from '$lib/components/segmented_buttons.svelte';
	import Toggle from '$lib/components/toggle.svelte';
	import { Tooltip } from '$lib/modules/overlay/ui';

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

	let unlisten: UnlistenFn | undefined;
	let unlistenChoose: (() => void) | undefined;
	onMount(async () => {
		unlistenChoose = onNodeAction(id, 'chooseFile', () => {
			chooseFile().catch(() => {});
		});
		unlisten = await listen<ProgressEvent>('audio://recorder_progress', (e) => {
			const p = e.payload;
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
	});

	$effect(() => {
		if (audioStore.isRunning) {
			if (untrack(() => committedFormat) === null) {
				committedFormat = untrack(() => data.format);
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
		}
	});

	$effect(() => {
		if (audioStore.chooseFileNodeId !== id) return;
		audioStore.chooseFileNodeId = null;
		const retryId = audioStore.pendingRetryPipelineId;
		audioStore.pendingRetryPipelineId = null;
		chooseFile().then((picked) => {
			if (!picked || retryId === null) return;
			const snapshot = pipelineStore.editorActions?.getSnapshot();
			if (!snapshot) return;
			const nodes = snapshot.nodes.map((n) =>
				n.id === id ? { ...n, data: { ...n.data, allowOverwrite: true } } : n
			);
			audioStore
				.activatePipeline(retryId, { nodes, edges: snapshot.edges })
				.catch((e) => audioStore.reportError(e));
		}).catch(() => {});
	});

	onDestroy(() => {
		unlisten?.();
		unlistenChoose?.();
	});

	// Mirrors `RecordingFormat::max_channels`; the encoder rejects anything wider.
	function maxChannelsFor(fmt: RecordingFormat): number {
		if (fmt.kind === 'mp3' || fmt.kind === 'opus') return 2;
		if (fmt.kind === 'flac') return 8;
		if (fmt.kind === 'aac') return 48;
		return 512;
	}

	const FORMATS = [
		{ value: 'wav' as const, label: 'WAV' },
		{ value: 'flac' as const, label: 'FLAC' },
		{ value: 'aiff' as const, label: 'AIFF' },
		{ value: 'opus' as const, label: 'Opus' },
		{ value: 'mp3' as const, label: 'MP3' },
		{ value: 'aac' as const, label: 'AAC' }
	];

	let maxChannels = $derived(maxChannelsFor(data.format));

	let CHANNEL_MODES = $derived([
		{ value: 'mono' as const, label: 'Mono' },
		{ value: 'stereo' as const, label: 'Stereo' },
		{ value: 'multi' as const, label: 'Multi', disabled: maxChannels <= 2 }
	]);

	type ChannelMode = 'mono' | 'stereo' | 'multi';
	let channelMode = $derived<ChannelMode>(
		data.channels <= 1 ? 'mono' : data.channels === 2 ? 'stereo' : 'multi'
	);

	let channelLabel = $derived(
		data.channels <= 1 ? 'mono' : data.channels === 2 ? 'stereo' : `${data.channels} ch`
	);

	function setChannelMode(mode: ChannelMode) {
		const target = mode === 'mono' ? 1 : mode === 'stereo' ? 2 : Math.max(3, data.channels);
		dropEdgesAbove(Math.min(target, maxChannels));
		flow.updateNodeData(id, { channels: Math.min(target, maxChannels) });
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
	let slotCap = $derived(
		channelMode === 'mono' ? 1 : channelMode === 'stereo' ? 2 : maxChannels
	);

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

	function extension(fmt: RecordingFormat): string {
		if (fmt.kind === 'flac') return 'flac';
		if (fmt.kind === 'opus') return 'opus';
		if (fmt.kind === 'mp3') return 'mp3';
		if (fmt.kind === 'aac') return 'm4a';
		if (fmt.kind === 'aiff') return 'aiff';
		return 'wav';
	}

	async function chooseFile(): Promise<boolean> {
		const ext = extension(data.format);
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

	function setFormatKind(kind: 'wav' | 'flac' | 'opus' | 'mp3' | 'aac' | 'aiff') {
		if (data.format.kind === kind) return;
		let next: RecordingFormat;
		if (kind === 'wav') next = { kind: 'wav', bitDepth: 'f32' };
		else if (kind === 'flac') next = { kind: 'flac', bitDepth: 'i24', compression: 'default' };
		else if (kind === 'opus') next = { kind: 'opus', bitrate: 128_000, application: 'audio' };
		else if (kind === 'mp3') next = { kind: 'mp3', bitrateKbps: 192 };
		else if (kind === 'aac') next = { kind: 'aac', bitrate: 192_000 };
		else next = { kind: 'aiff', bitDepth: 'i24' };
		const patch: Partial<FileRecordingNodeData> = { format: next };
		if (data.filePath && !audioStore.isRunning) {
			patch.filePath = replaceExtension(data.filePath, extension(next));
		}
		flow.updateNodeData(id, patch);
	}

	function setWavBitDepth(bd: WavBitDepth) {
		if (data.format.kind !== 'wav') return;
		flow.updateNodeData(id, { format: { kind: 'wav', bitDepth: bd } });
	}

	function setFlacBitDepth(bd: FlacBitDepth) {
		if (data.format.kind !== 'flac') return;
		flow.updateNodeData(id, {
			format: { kind: 'flac', bitDepth: bd, compression: data.format.compression }
		});
	}

	function setFlacCompression(c: FlacCompression) {
		if (data.format.kind !== 'flac') return;
		flow.updateNodeData(id, {
			format: { kind: 'flac', bitDepth: data.format.bitDepth, compression: c }
		});
	}

	function setOpusBitrate(bps: number) {
		if (data.format.kind !== 'opus') return;
		flow.updateNodeData(id, {
			format: { kind: 'opus', bitrate: bps, application: data.format.application }
		});
	}

	function setOpusApplication(a: OpusApplication) {
		if (data.format.kind !== 'opus') return;
		flow.updateNodeData(id, {
			format: { kind: 'opus', bitrate: data.format.bitrate, application: a }
		});
	}

	function setMp3Bitrate(kbps: number) {
		if (data.format.kind !== 'mp3') return;
		flow.updateNodeData(id, { format: { kind: 'mp3', bitrateKbps: kbps } });
	}

	function setAacBitrate(bps: number) {
		if (data.format.kind !== 'aac') return;
		flow.updateNodeData(id, { format: { kind: 'aac', bitrate: bps } });
	}

	function setAiffBitDepth(bd: AiffBitDepth) {
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

	const OPUS_BITRATE_PRESETS: { kbps: number; label: string }[] = [
		{ kbps: 64, label: '64' },
		{ kbps: 96, label: '96' },
		{ kbps: 128, label: '128' },
		{ kbps: 192, label: '192' },
		{ kbps: 256, label: '256' }
	];

	const MP3_BITRATE_PRESETS: { kbps: number; label: string }[] = [
		{ kbps: 128, label: '128' },
		{ kbps: 192, label: '192' },
		{ kbps: 256, label: '256' },
		{ kbps: 320, label: '320' }
	];

	const AAC_BITRATE_PRESETS: { kbps: number; label: string }[] = [
		{ kbps: 96, label: '96' },
		{ kbps: 128, label: '128' },
		{ kbps: 192, label: '192' },
		{ kbps: 256, label: '256' }
	];

	const AIFF_BIT_DEPTHS: { value: AiffBitDepth; label: string }[] = [
		{ value: 'i16', label: '16-bit' },
		{ value: 'i24', label: '24-bit' }
	];

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
			return Math.round((data.format.bitrateKbps * 1000 / 8) * seconds + 512);
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
	let dirty = $derived(
		recording &&
		committedFormat !== null &&
		JSON.stringify(committedFormat) !== JSON.stringify(data.format)
	);
</script>

<Wrapper label="File Recording" icon={FileRecord} accent="output" hasInput channelIo nodeId={id} maxChannels={slotCap}>
	<div class="flex w-64 flex-col gap-1.5">
		<div
			class="truncate rounded bg-neutral-100 px-2 py-1 text-xs text-neutral-1000"
			title={data.filePath ?? undefined}
		>
			{basename(data.filePath)}
		</div>
		<div class="flex gap-1">
			<button
				type="button"
				class="nodrag nopan button-main primary flex h-7 flex-1 items-center justify-center gap-1.5 rounded-lg py-0 text-xs"
				onclick={chooseFile}
			>
				<Folder class="size-3.5" />
				Choose file
			</button>
			<Tooltip text="Reveal the recording in Finder">
				<button
					type="button"
					class="nodrag nopan button-main primary size-7 shrink-0 rounded-lg p-0"
					disabled={!data.filePath}
					onclick={revealFolder}
				>
					<FolderOpen class="size-3.5" />
				</button>
			</Tooltip>
		</div>
		<Toggle
			size="sm"
			label="Allow overwrite"
			checked={data.allowOverwrite}
			onChange={(v) => flow.updateNodeData(id, { allowOverwrite: v })}
		/>

		<SegmentedButtons
			options={FORMATS}
			value={data.format.kind}
			onSelect={setFormatKind}
			columns={3}
		/>

		<SegmentedButtons
			label="Channels"
			note={maxChannels >= 512 ? 'no limit' : `max ${maxChannels}`}
			options={CHANNEL_MODES}
			value={channelMode}
			onSelect={setChannelMode}
		/>

		{#if data.format.kind === 'wav'}
			<SegmentedButtons
				options={WAV_BIT_DEPTHS.map((b) => ({ value: b.value, label: b.label, subtitle: b.sub }))}
				value={data.format.bitDepth}
				onSelect={setWavBitDepth}
			/>
		{:else if data.format.kind === 'flac'}
			<SegmentedButtons
				options={FLAC_BIT_DEPTHS}
				value={data.format.bitDepth}
				onSelect={setFlacBitDepth}
			/>
			<SegmentedButtons
				options={FLAC_COMPRESSIONS}
				value={data.format.compression}
				onSelect={setFlacCompression}
			/>
		{:else if data.format.kind === 'opus'}
			<SegmentedButtons
				label="Bitrate"
				note="kbps"
				options={OPUS_BITRATE_PRESETS.map((p) => ({ value: p.kbps * 1000, label: p.label }))}
				value={data.format.bitrate}
				onSelect={setOpusBitrate}
			/>
			<SegmentedButtons
				options={OPUS_APPLICATIONS.map((a) => ({ value: a.value, label: a.label, subtitle: a.sub }))}
				value={data.format.application}
				onSelect={setOpusApplication}
			/>
		{:else if data.format.kind === 'mp3'}
			<SegmentedButtons
				label="Bitrate"
				note="kbps"
				options={MP3_BITRATE_PRESETS.map((p) => ({ value: p.kbps, label: p.label }))}
				value={data.format.bitrateKbps}
				onSelect={setMp3Bitrate}
			/>
			<div class="text-center font-mono text-[9px] text-neutral-600">CBR</div>
		{:else if data.format.kind === 'aac'}
			<SegmentedButtons
				label="Bitrate"
				note="kbps"
				options={AAC_BITRATE_PRESETS.map((p) => ({ value: p.kbps * 1000, label: p.label }))}
				value={data.format.bitrate}
				onSelect={setAacBitrate}
			/>
			<div class="text-center font-mono text-[9px] text-neutral-600">M4A</div>
		{:else}
			<SegmentedButtons
				options={AIFF_BIT_DEPTHS}
				value={data.format.bitDepth}
				onSelect={setAiffBitDepth}
			/>
			<div class="text-center font-mono text-[9px] text-neutral-600">PCM big-endian</div>
		{/if}

		<div class="flex items-baseline justify-between font-mono text-[11px]">
			<span class={recording ? 'text-red-500' : 'text-neutral-900'}>
				{recording ? '● REC' : '○'}
			</span>
			<span class="text-neutral-1000 tabular-nums">{formatDuration(durationSec)}</span>
		</div>
		<div class="flex justify-between text-[10px] text-neutral-900">
			<span>{formatLabelFor(recording && committedFormat !== null ? committedFormat : data.format)} · {channelLabel}</span>
			<span class="font-mono tabular-nums">{formatSize(estSize)}</span>
		</div>
		{#if dirty}
			<div class="text-[9px] text-amber-600">changes pending - restart or choose new file</div>
		{/if}
	</div>
</Wrapper>
