<script lang="ts">
	import { open } from '@tauri-apps/plugin-dialog';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { AudioFileNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { Rewind, Loop } from '$lib/components/icons';
	import { onNodeAction } from '$lib/modules/flow/utils';

	type AudioFileNodeType = Node<AudioFileNodeData, 'audioFile'>;
	let { id, data }: NodeProps<AudioFileNodeType> = $props();

	const flow = useSvelteFlow();

	interface ProgressEvent {
		nodeId: string;
		frames: number;
		totalFrames: number;
		sampleRate: number;
		stopped: boolean;
	}

	let frames = $state(0);
	let totalFrames = $state(0);
	let sampleRate = $state(0);
	let playing = $state(false);

	let unlisten: UnlistenFn | undefined;
	let unlistenChoose: (() => void) | undefined;
	onMount(async () => {
		unlistenChoose = onNodeAction(id, 'chooseFile', () => {
			chooseFile().catch(() => {});
		});
		unlisten = await listen<ProgressEvent>('audio://audio_file_progress', (e) => {
			const p = e.payload;
			if (p.nodeId !== id) return;
			frames = p.frames;
			totalFrames = p.totalFrames;
			sampleRate = p.sampleRate;
			playing = !p.stopped;
		});
	});

	$effect(() => {
		if (!audioStore.isRunning) {
			playing = false;
		}
	});

	// Toggle is a runtime atomic, not part of InputSpec -- syncing here
	// keeps the reader's loop flag in lockstep with `data.loopEnabled`
	// without a reconcile.
	$effect(() => {
		if (audioStore.isRunning && data.filePath) {
			audioMethods.setAudioFileLoop(id, data.loopEnabled).catch(() => {});
		}
	});

	onDestroy(() => {
		unlisten?.();
		unlistenChoose?.();
	});

	async function chooseFile() {
		const path = await open({
			title: 'Pick audio file',
			multiple: false,
			directory: false,
			filters: [{ name: 'WAV', extensions: ['wav'] }]
		});
		if (typeof path === 'string') {
			flow.updateNodeData(id, { filePath: path });
		}
	}

	function rewind() {
		if (!audioStore.isRunning) return;
		audioMethods.seekAudioFile(id, 0).catch(() => {});
	}

	function toggleLoop() {
		flow.updateNodeData(id, { loopEnabled: !data.loopEnabled });
	}

	function onScrub(e: Event) {
		const target = e.target as HTMLInputElement;
		const target_frame = Number(target.value);
		if (!Number.isFinite(target_frame)) return;
		frames = target_frame;
		if (audioStore.isRunning) {
			audioMethods.seekAudioFile(id, target_frame).catch(() => {});
		}
	}

	function basename(p: string | null): string {
		if (!p) return 'No file selected';
		const i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
		return i >= 0 ? p.slice(i + 1) : p;
	}

	function formatTime(sec: number): string {
		if (!Number.isFinite(sec) || sec < 0) sec = 0;
		const minutes = Math.floor(sec / 60);
		const remainder = sec - minutes * 60;
		return `${minutes}:${remainder.toFixed(1).padStart(4, '0')}`;
	}

	let currentSec = $derived(sampleRate > 0 ? frames / sampleRate : 0);
	let totalSec = $derived(sampleRate > 0 ? totalFrames / sampleRate : 0);
</script>

<Wrapper label="Audio File" accent="input" hasOutput>
	<div class="flex w-64 flex-col gap-1.5">
		<div
			class="truncate rounded bg-neutral-100 px-2 py-1 text-xs text-neutral-1000"
			title={data.filePath ?? undefined}
		>
			{basename(data.filePath)}
		</div>

		<div class="flex gap-1">
			<button class="button-main primary nodrag nopan flex-1 py-1 text-xs" onclick={chooseFile}>
				Choose file...
			</button>
			<button
				type="button"
				class="nodrag nopan flex h-7 w-7 shrink-0 items-center justify-center rounded border border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200 disabled:opacity-40"
				title="Rewind to start"
				disabled={!audioStore.isRunning || !data.filePath}
				onclick={rewind}
			>
				<Rewind class="h-3.5 w-3.5" />
			</button>
			<button
				type="button"
				class={[
					'nodrag nopan flex h-7 w-7 shrink-0 items-center justify-center rounded border text-xs transition-colors',
					data.loopEnabled
						? 'border-neutral-900 bg-neutral-900 text-white'
						: 'border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
				]}
				title={data.loopEnabled ? 'Loop on' : 'Loop off'}
				onclick={toggleLoop}
			>
				<Loop class="h-3.5 w-3.5" />
			</button>
		</div>

		<input
			type="range"
			class="nodrag nopan h-1 w-full cursor-pointer accent-neutral-900 disabled:opacity-40"
			min="0"
			max={Math.max(totalFrames, 1)}
			value={frames}
			disabled={!data.filePath || totalFrames === 0}
			oninput={onScrub}
		/>

		<div class="flex items-baseline justify-between font-mono text-[11px]">
			<span class={playing ? 'text-green-600' : 'text-neutral-900'}>
				{playing ? '> PLAY' : '||'}
			</span>
			<span class="text-neutral-1000 tabular-nums">
				{formatTime(currentSec)} / {formatTime(totalSec)}
			</span>
		</div>
	</div>
</Wrapper>
