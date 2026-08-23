<script lang="ts">
	import { open } from '@tauri-apps/plugin-dialog';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { AudioFileNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from '../effect/_slider.svelte';
	import { Autoplay, Folder, Loop, MusicNote, Pause, Play, SkipBack5, SkipForward5, Stop } from '$lib/components/icons';
	import { onNodeAction } from '$lib/modules/flow/utils';
	import { Tooltip } from '$lib/modules/overlay/ui';

	type AudioFileNodeType = Node<AudioFileNodeData, 'audioFile'>;
	let { id, data }: NodeProps<AudioFileNodeType> = $props();

	const flow = useSvelteFlow();

	interface ProgressEvent {
		nodeId: string;
		frames: number;
		totalFrames: number;
		sampleRate: number;
		channels: number;
		stopped: boolean;
		paused: boolean;
	}

	let frames = $state(0);
	let totalFrames = $state(0);
	let sampleRate = $state(0);
	// Reported by the reader once the file is open; drives the socket count.
	let channels = $state(0);
	let playing = $state(false);
	let paused = $state(false);
	// While the user drags the scrubber, this holds the hand position so the
	// thumb follows the cursor instead of being yanked back by the 100 ms
	// progress events; cleared on release to resume live updates.
	let scrubValue: number | null = $state(null);

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
			channels = p.channels;
			paused = p.paused;
			playing = !p.stopped && !p.paused;
			// Re-assert the loop flag on every tick: the reader starts with loop
			// disabled and only a fresh reader reports frames 0, which can arrive
			// before `isRunning` flips. Keeping it in sync here guarantees the
			// reader sees the intended value well before EOF. Idempotent store.
			if (audioStore.isRunning && data.filePath) {
				audioMethods.setAudioFileLoop(id, data.loopEnabled).catch(() => {});
			}
		});
	});

	$effect(() => {
		if (!audioStore.isRunning) {
			playing = false;
			paused = false;
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
			filters: [{ name: 'Audio', extensions: ['wav', 'flac', 'aif', 'aiff', 'mp3', 'm4a', 'aac', 'opus', 'ogg'] }]
		});
		if (typeof path === 'string') {
			flow.updateNodeData(id, { filePath: path });
		}
	}

	function togglePlayPause() {
		if (!audioStore.isRunning) return;
		if (paused || !playing) {
			paused = false;
			playing = true;
			audioMethods.setAudioFilePaused(id, false).catch(() => {});
		} else {
			paused = true;
			playing = false;
			audioMethods.setAudioFilePaused(id, true).catch(() => {});
		}
	}

	function stop() {
		if (!audioStore.isRunning) return;
		paused = true;
		playing = false;
		audioMethods.seekAudioFile(id, 0).catch(() => {});
		audioMethods.setAudioFilePaused(id, true).catch(() => {});
	}

	function skipBack() {
		if (!audioStore.isRunning || !data.filePath) return;
		const target = Math.max(0, frames - 5 * sampleRate);
		frames = target;
		audioMethods.seekAudioFile(id, target).catch(() => {});
	}

	function skipForward() {
		if (!audioStore.isRunning || !data.filePath) return;
		const target = Math.min(totalFrames, frames + 5 * sampleRate);
		frames = target;
		audioMethods.seekAudioFile(id, target).catch(() => {});
	}

	function toggleLoop() {
		flow.updateNodeData(id, { loopEnabled: !data.loopEnabled });
	}

	function toggleAutoStart() {
		flow.updateNodeData(id, { autoStart: !data.autoStart });
	}

	function onScrub(e: Event) {
		const target = e.target as HTMLInputElement;
		const target_frame = Number(target.value);
		if (!Number.isFinite(target_frame)) return;
		scrubValue = target_frame;
		frames = target_frame;
		if (audioStore.isRunning) {
			audioMethods.seekAudioFile(id, target_frame).catch(() => {});
		}
	}

	function clearScrub() {
		scrubValue = null;
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

	function formatRate(hz: number): string {
		return hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)} kHz` : `${hz} Hz`;
	}

	function extension(p: string | null): string {
		const i = p?.lastIndexOf('.') ?? -1;
		return i > 0 ? (p as string).slice(i + 1).toUpperCase() : '';
	}

	let channelLabel = $derived(channels <= 1 ? 'mono' : channels === 2 ? 'stereo' : `${channels} ch`);

	let currentSec = $derived(sampleRate > 0 ? frames / sampleRate : 0);
	let totalSec = $derived(sampleRate > 0 ? totalFrames / sampleRate : 0);
	let canControl = $derived(audioStore.isRunning && !!data.filePath);
	let missing = $derived(!!data.filePath && audioStore.missingFilePaths.has(data.filePath));

	function setVolume(pct: number) {
		const scalar = Math.max(0, Math.min(1, pct / 100));
		flow.updateNodeData(id, { volume: scalar });
		audioMethods.setInputVolume(id, scalar).catch(() => {});
	}

	function formatPct(p: number): string {
		return `${Math.round(p)}%`;
	}

	let volumePct = $derived((data.volume ?? 1) * 100);
</script>

<Wrapper
	label="Audio File"
	accent="input"
	icon={MusicNote}
	hasOutput
	channelIo
	nodeId={id}
	minChannels={channels}
	maxChannels={channels || undefined}
	selfGrowing>
	<div class="flex w-64 flex-col gap-3">
		<div class="truncate rounded bg-neutral-100 px-2 py-1 text-xs text-neutral-1000" title={data.filePath ?? undefined}>
			{basename(data.filePath)}
		</div>

		{#if missing}
			<span class="text-[10px] text-red-500">File not found</span>
		{/if}

		{#if sampleRate > 0}
			<div class="flex justify-between text-[10px] text-neutral-900">
				<span class="font-mono">{formatRate(sampleRate)} · {channelLabel}</span>
				<span class="font-mono tabular-nums">{extension(data.filePath)}</span>
			</div>
		{/if}

		<div class="flex items-center gap-1">
			<button
				type="button"
				class="nodrag nopan button-main primary flex h-7 flex-1 items-center justify-center gap-1.5 rounded-lg py-0 text-xs"
				onclick={chooseFile}>
				<Folder class="size-3.5" />
				Choose file
			</button>
			<Tooltip text="Start playback automatically when the pipeline runs">
				<button
					type="button"
					class={['nodrag nopan button-main primary size-7 rounded-lg p-0', (data.autoStart ?? true) && 'active']}
					onclick={toggleAutoStart}>
					<Autoplay class="size-3.5" />
				</button>
			</Tooltip>
		</div>

		<input
			type="range"
			class="nodrag nopan nowheel h-1 w-full cursor-pointer accent-neutral-900 disabled:opacity-40"
			min="0"
			max={Math.max(totalFrames, 1)}
			value={scrubValue ?? frames}
			disabled={!data.filePath || totalFrames === 0}
			oninput={onScrub}
			onpointerup={clearScrub}
			onpointercancel={clearScrub}
			onkeyup={clearScrub} />

		<div class="flex items-center justify-between font-mono text-[11px]">
			<span class="text-neutral-900 tabular-nums">{formatTime(currentSec)}</span>
			<div class="flex items-center justify-center gap-1">
				<Tooltip text="Stop and rewind to start">
					<button type="button" class="nodrag nopan button-main primary size-6 rounded-lg p-0" disabled={!canControl} onclick={stop}>
						<Stop class="size-3" />
					</button>
				</Tooltip>
				<Tooltip text="Skip back 5 seconds">
					<button type="button" class="nodrag nopan button-main primary size-6 rounded-lg p-0" disabled={!canControl} onclick={skipBack}>
						<SkipBack5 class="size-3" />
					</button>
				</Tooltip>
				<Tooltip text={playing ? 'Pause playback' : 'Play'}>
					<button type="button" class="nodrag nopan button-main primary size-6 rounded-lg p-0" disabled={!canControl} onclick={togglePlayPause}>
						{#if playing}
							<Pause class="size-3" />
						{:else}
							<Play class="size-3" />
						{/if}
					</button>
				</Tooltip>
				<Tooltip text="Skip forward 5 seconds">
					<button type="button" class="nodrag nopan button-main primary size-6 rounded-lg p-0" disabled={!canControl} onclick={skipForward}>
						<SkipForward5 class="size-3" />
					</button>
				</Tooltip>
				<Tooltip text={data.loopEnabled ? 'Looping on: restart at end' : 'Loop off: stop at end'}>
					<button type="button" class={['nodrag nopan button-main primary size-6 rounded-lg p-0', data.loopEnabled && 'active']} onclick={toggleLoop}>
						<Loop class="size-3" />
					</button>
				</Tooltip>
			</div>
			<span class="text-neutral-900 tabular-nums">{formatTime(totalSec)}</span>
		</div>

		<Slider label="Volume" value={volumePct} min={0} max={100} step={1} format={formatPct} defaultValue={100} ticks={[25, 50, 75]} onChange={setVolume} />
	</div>
</Wrapper>
