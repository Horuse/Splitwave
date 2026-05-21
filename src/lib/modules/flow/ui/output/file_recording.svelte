<script lang="ts">
	import { save } from '@tauri-apps/plugin-dialog';
	import { openPath } from '@tauri-apps/plugin-opener';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount, untrack } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
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
	import { Folder } from '$lib/components/icons';
	import { onNodeAction } from '$lib/modules/flow/utils';

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

	function dirname(p: string): string {
		const idx = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
		return idx > 0 ? p.slice(0, idx) : p;
	}

	async function revealFolder() {
		if (!data.filePath) return;
		try {
			await openPath(dirname(data.filePath));
		} catch {
			// silent fail
		}
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

<Wrapper label="File Recording" accent="output" hasInput>
	<div class="flex w-64 flex-col gap-1.5">
		<div
			class="truncate rounded bg-neutral-100 px-2 py-1 text-xs text-neutral-1000"
			title={data.filePath ?? undefined}
		>
			{basename(data.filePath)}
		</div>
		<div class="flex gap-1">
			<button class="button-main primary rounded-lg nodrag nopan flex-1 py-1 text-xs" onclick={chooseFile}>
				Choose file…
			</button>
			<button
				type="button"
				class="nodrag nopan flex h-7 w-7 shrink-0 items-center justify-center rounded border border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200 disabled:opacity-40"
				title="Reveal in folder"
				disabled={!data.filePath}
				onclick={revealFolder}
			>
				<Folder class="h-3.5 w-3.5" />
			</button>
		</div>
		<label class="nodrag nopan flex cursor-pointer items-center gap-1.5 text-[10px] text-neutral-700">
			<input
				type="checkbox"
				class="accent-neutral-900"
				checked={data.allowOverwrite}
				onchange={(e) => flow.updateNodeData(id, { allowOverwrite: e.currentTarget.checked })}
			/>
			Allow overwrite
		</label>

		<div class="nodrag nopan grid grid-cols-6 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
			{#each [{ k: 'wav' as const, label: 'WAV' }, { k: 'flac' as const, label: 'FLAC' }, { k: 'aiff' as const, label: 'AIFF' }, { k: 'opus' as const, label: 'Opus' }, { k: 'mp3' as const, label: 'MP3' }, { k: 'aac' as const, label: 'AAC' }] as fmt (fmt.k)}
				<button
					type="button"
					onclick={() => setFormatKind(fmt.k)}
					class={[
						'rounded-sm py-0.5 font-mono text-[10px] leading-none transition-colors',
						data.format.kind === fmt.k
							? 'bg-neutral-900 text-white'
							: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
					]}
				>
					{fmt.label}
				</button>
			{/each}
		</div>

		{#if data.format.kind === 'wav'}
			<div class="nodrag nopan grid grid-cols-3 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each WAV_BIT_DEPTHS as bd (bd.value)}
					<button
						type="button"
						onclick={() => setWavBitDepth(bd.value)}
						class={[
							'flex flex-col items-center rounded-sm py-0.5 leading-none transition-colors',
							data.format.kind === 'wav' && data.format.bitDepth === bd.value
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						<span class="font-mono text-[10px] tabular-nums">{bd.label}</span>
						<span class="text-[8px] opacity-70">{bd.sub}</span>
					</button>
				{/each}
			</div>
		{:else if data.format.kind === 'flac'}
			<div class="nodrag nopan grid grid-cols-2 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each FLAC_BIT_DEPTHS as bd (bd.value)}
					<button
						type="button"
						onclick={() => setFlacBitDepth(bd.value)}
						class={[
							'rounded-sm py-0.5 font-mono text-[10px] leading-none transition-colors',
							data.format.kind === 'flac' && data.format.bitDepth === bd.value
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						{bd.label}
					</button>
				{/each}
			</div>
			<div class="nodrag nopan grid grid-cols-3 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each FLAC_COMPRESSIONS as c (c.value)}
					<button
						type="button"
						onclick={() => setFlacCompression(c.value)}
						class={[
							'rounded-sm py-0.5 text-[10px] leading-none transition-colors',
							data.format.kind === 'flac' && data.format.compression === c.value
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						{c.label}
					</button>
				{/each}
			</div>
		{:else if data.format.kind === 'opus'}
			<div class="nodrag nopan grid grid-cols-5 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each OPUS_BITRATE_PRESETS as p (p.kbps)}
					<button
						type="button"
						onclick={() => setOpusBitrate(p.kbps * 1000)}
						class={[
							'rounded-sm py-0.5 font-mono text-[10px] leading-none transition-colors',
							data.format.kind === 'opus' && data.format.bitrate === p.kbps * 1000
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						{p.label}
					</button>
				{/each}
			</div>
			<div class="text-center font-mono text-[9px] text-neutral-600">kbps</div>
			<div class="nodrag nopan grid grid-cols-3 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each OPUS_APPLICATIONS as a (a.value)}
					<button
						type="button"
						onclick={() => setOpusApplication(a.value)}
						class={[
							'flex flex-col items-center rounded-sm py-0.5 leading-none transition-colors',
							data.format.kind === 'opus' && data.format.application === a.value
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						<span class="text-[10px]">{a.label}</span>
						<span class="text-[8px] opacity-70">{a.sub}</span>
					</button>
				{/each}
			</div>
		{:else if data.format.kind === 'mp3'}
			<div class="nodrag nopan grid grid-cols-4 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each MP3_BITRATE_PRESETS as p (p.kbps)}
					<button
						type="button"
						onclick={() => setMp3Bitrate(p.kbps)}
						class={[
							'rounded-sm py-0.5 font-mono text-[10px] leading-none transition-colors',
							data.format.kind === 'mp3' && data.format.bitrateKbps === p.kbps
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						{p.label}
					</button>
				{/each}
			</div>
			<div class="text-center font-mono text-[9px] text-neutral-600">kbps · CBR</div>
		{:else if data.format.kind === 'aac'}
			<div class="nodrag nopan grid grid-cols-4 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each AAC_BITRATE_PRESETS as p (p.kbps)}
					<button
						type="button"
						onclick={() => setAacBitrate(p.kbps * 1000)}
						class={[
							'rounded-sm py-0.5 font-mono text-[10px] leading-none transition-colors',
							data.format.kind === 'aac' && data.format.bitrate === p.kbps * 1000
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						{p.label}
					</button>
				{/each}
			</div>
			<div class="text-center font-mono text-[9px] text-neutral-600">kbps · M4A</div>
		{:else}
			<div class="nodrag nopan grid grid-cols-2 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
				{#each AIFF_BIT_DEPTHS as bd (bd.value)}
					<button
						type="button"
						onclick={() => setAiffBitDepth(bd.value)}
						class={[
							'rounded-sm py-0.5 font-mono text-[10px] leading-none transition-colors',
							data.format.kind === 'aiff' && data.format.bitDepth === bd.value
								? 'bg-neutral-900 text-white'
								: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
						]}
					>
						{bd.label}
					</button>
				{/each}
			</div>
			<div class="text-center font-mono text-[9px] text-neutral-600">PCM big-endian</div>
		{/if}

		<div class="flex items-baseline justify-between font-mono text-[11px]">
			<span class={recording ? 'text-red-500' : 'text-neutral-900'}>
				{recording ? '● REC' : '○'}
			</span>
			<span class="text-neutral-1000 tabular-nums">{formatDuration(durationSec)}</span>
		</div>
		<div class="flex justify-between text-[10px] text-neutral-900">
			<span>{formatLabelFor(recording && committedFormat !== null ? committedFormat : data.format)} · stereo</span>
			<span class="font-mono tabular-nums">{formatSize(estSize)}</span>
		</div>
		{#if dirty}
			<div class="text-[9px] text-amber-600">changes pending - restart or choose new file</div>
		{/if}
	</div>
</Wrapper>
