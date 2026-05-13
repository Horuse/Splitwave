<script lang="ts">
	import { save } from '@tauri-apps/plugin-dialog';
	import { openPath } from '@tauri-apps/plugin-opener';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { FileRecordingNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import Wrapper from '../node.svelte';

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

	let unlisten: UnlistenFn | undefined;
	onMount(async () => {
		unlisten = await listen<ProgressEvent>('audio://recorder_progress', (e) => {
			const p = e.payload;
			if (p.nodeId !== id) return;
			frames = p.frames;
			sampleRate = p.sampleRate;
			recording = !p.stopped;
		});
	});

	$effect(() => {
		if (!audioStore.isRunning) {
			recording = false;
		}
	});

	onDestroy(() => unlisten?.());

	async function chooseFile() {
		const path = await save({
			title: 'Save recording',
			filters: [{ name: 'WAV (32-bit float)', extensions: ['wav'] }]
		});
		if (path) flow.updateNodeData(id, { filePath: path });
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

	// f32 stereo = 8 bytes/frame; +44 for the RIFF header.
	let bytesPerFrame = 4 * 2;
	let estimatedSize = $derived(frames * bytesPerFrame + 44);
	let durationSec = $derived(sampleRate > 0 ? frames / sampleRate : 0);
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
			<button class="button-main primary nodrag nopan flex-1 py-1 text-xs" onclick={chooseFile}>
				Choose file…
			</button>
			<button
				type="button"
				class="nodrag nopan flex h-7 w-7 shrink-0 items-center justify-center rounded border border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200 disabled:opacity-40"
				title="Reveal in folder"
				disabled={!data.filePath}
				onclick={revealFolder}
			>
				<svg viewBox="0 0 16 16" class="h-3.5 w-3.5" aria-hidden="true">
					<path
						d="M1.5 5h4l1.5-1.5h6A1.5 1.5 0 0 1 14.5 5v6A1.5 1.5 0 0 1 13 12.5H3A1.5 1.5 0 0 1 1.5 11V5Z"
						fill="none"
						stroke="currentColor"
						stroke-width="1.2"
						stroke-linejoin="round"
					/>
				</svg>
			</button>
		</div>
		<div class="flex items-baseline justify-between font-mono text-[11px]">
			<span class={recording ? 'text-red-500' : 'text-neutral-900'}>
				{recording ? '● REC' : '○'}
			</span>
			<span class="text-neutral-1000 tabular-nums">{formatDuration(durationSec)}</span>
		</div>
		<div class="flex justify-between text-[10px] text-neutral-900">
			<span>WAV PCM 32-bit float · stereo</span>
			<span class="font-mono tabular-nums">{formatSize(estimatedSize)}</span>
		</div>
	</div>
</Wrapper>
