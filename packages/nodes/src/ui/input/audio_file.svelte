<script lang="ts">
	import { Button, Slider, Tooltip } from '@splitwave/ui';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { AudioFileNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import ActionSocket from '../_action_socket.svelte';
	import Folder from '~icons/fluent/folder-16-filled';
	import Loop from '~icons/fluent/arrow-repeat-all-16-filled';
	import MusicNote from '~icons/fluent/music-note-2-16-filled';
	import Pause from '~icons/fluent/pause-24-filled';
	import Play from '~icons/fluent/play-24-filled';
	import Stop from '~icons/fluent/stop-24-filled';
	import { Autoplay, SkipBack5, SkipForward5 } from '../icons';
	import { onNodeAction } from '../utils';
	import { formatHz, formatDuration, formatPct } from '@splitwave/nodes';
	import { getEnginePort } from '../../engine';

	const engine = getEnginePort();

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

	let unlistenChoose: (() => void) | undefined;
	unlistenChoose = onNodeAction(id, 'chooseFile', () => {
		chooseFile().catch(() => {});
	});
	engine.listen<ProgressEvent>('audio://audio_file_progress', (p) => {
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
		if (engine.isRunning && data.filePath) {
			engine.audio.setAudioFileLoop(id, data.loopEnabled).catch(() => {});
		}
	});

	$effect(() => {
		if (!engine.isRunning) {
			playing = false;
			paused = false;
		}
	});

	// Toggle is a runtime atomic, not part of InputSpec -- syncing here
	// keeps the reader's loop flag in lockstep with `data.loopEnabled`
	// without a reconcile.
	$effect(() => {
		if (engine.isRunning && data.filePath) {
			engine.audio.setAudioFileLoop(id, data.loopEnabled).catch(() => {});
		}
	});

	onDestroy(() => {
		unlistenChoose?.();
	});

	async function chooseFile() {
		const path = await engine.pickFile({
			title: 'Pick audio file',
			directory: false,
			filters: [{ name: 'Audio', extensions: ['wav', 'flac', 'aif', 'aiff', 'mp3', 'm4a', 'aac', 'opus', 'ogg'] }]
		});
		if (path) {
			flow.updateNodeData(id, { filePath: path });
		}
	}

	function togglePlayPause() {
		if (!engine.isRunning) return;
		if (paused || !playing) {
			paused = false;
			playing = true;
			engine.audio.setAudioFilePaused(id, false).catch(() => {});
		} else {
			paused = true;
			playing = false;
			engine.audio.setAudioFilePaused(id, true).catch(() => {});
		}
	}

	function stop() {
		if (!engine.isRunning) return;
		paused = true;
		playing = false;
		engine.audio.seekAudioFile(id, 0).catch(() => {});
		engine.audio.setAudioFilePaused(id, true).catch(() => {});
	}

	function skipBack() {
		if (!engine.isRunning || !data.filePath) return;
		const target = Math.max(0, frames - 5 * sampleRate);
		frames = target;
		engine.audio.seekAudioFile(id, target).catch(() => {});
	}

	function skipForward() {
		if (!engine.isRunning || !data.filePath) return;
		const target = Math.min(totalFrames, frames + 5 * sampleRate);
		frames = target;
		engine.audio.seekAudioFile(id, target).catch(() => {});
	}

	function toggleLoop() {
		flow.updateNodeData(id, { loopEnabled: !data.loopEnabled });
	}

	function toggleAutoStart() {
		flow.updateNodeData(id, { autoStart: !data.autoStart });
	}

	let scrubEl: HTMLDivElement | undefined = $state();
	let canScrub = $derived(!!data.filePath && totalFrames > 0);
	let playedPct = $derived(totalFrames > 0 ? Math.min(100, ((scrubValue ?? frames) / totalFrames) * 100) : 0);

	// The rail is inset by the thumb radius, like Slider, so the thumb never overhangs.
	function frameAt(clientX: number): number {
		const r = scrubEl!.getBoundingClientRect();
		const inner = r.width - 14;
		const ratio = inner > 0 ? (clientX - r.left - 7) / inner : 0;
		return Math.round(Math.max(0, Math.min(1, ratio)) * totalFrames);
	}

	function seekTo(target: number) {
		scrubValue = target;
		frames = target;
		if (engine.isRunning) engine.audio.seekAudioFile(id, target).catch(() => {});
	}

	function scrubDown(e: PointerEvent) {
		if (!canScrub || e.button !== 0) return;
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		seekTo(frameAt(e.clientX));
	}

	function scrubMove(e: PointerEvent) {
		if (!(e.currentTarget as HTMLElement).hasPointerCapture(e.pointerId)) return;
		seekTo(frameAt(e.clientX));
	}

	function scrubUp() {
		scrubValue = null;
	}

	function basename(p: string | null): string {
		if (!p) return 'No file selected';
		const i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
		return i >= 0 ? p.slice(i + 1) : p;
	}

	function extension(p: string | null): string {
		const i = p?.lastIndexOf('.') ?? -1;
		return i > 0 ? (p as string).slice(i + 1).toUpperCase() : '';
	}

	let channelLabel = $derived(channels <= 1 ? 'mono' : channels === 2 ? 'stereo' : `${channels} ch`);

	let currentSec = $derived(sampleRate > 0 ? frames / sampleRate : 0);
	let totalSec = $derived(sampleRate > 0 ? totalFrames / sampleRate : 0);
	let canControl = $derived(engine.isRunning && !!data.filePath);
	let missing = $derived(!!data.filePath && engine.missingFilePaths.has(data.filePath));

	function setVolume(pct: number) {
		const scalar = Math.max(0, Math.min(1, pct / 100));
		flow.updateNodeData(id, { volume: scalar });
		engine.audio.setInputVolume(id, scalar).catch(() => {});
	}

	let volumePct = $derived((data.volume ?? 1) * 100);

	let srcTooltip = $derived.by(() => {
		if (sampleRate <= 0 || sampleRate === engine.sampleRate) return undefined;
		return `Resampling: ${formatHz(sampleRate)} → ${formatHz(engine.sampleRate)}`;
	});
</script>

<Wrapper
	label="Audio File"
	accent="input"
	icon={MusicNote}
	{srcTooltip}
	hasOutput
	channelIo
	nodeId={id}
	minChannels={channels}
	maxChannels={channels || undefined}
	selfGrowing>
	<div class="flex w-64 flex-col gap-2">
		<!-- File: name over its format, picker and autostart beside it. -->
		<div class="flex items-center gap-2 rounded-lg border border-theme/5 bg-neutral-100 py-1.5 pr-1.5 pl-2.5">
			<div class="flex min-w-0 flex-1 flex-col gap-0.5">
				<span class={['truncate text-xs', data.filePath ? 'text-neutral-1100' : 'text-neutral-700']} title={data.filePath ?? undefined}>
					{basename(data.filePath)}
				</span>
				{#if missing}
					<span class="text-[10px] text-red-500">File not found</span>
				{:else if data.filePath}
					<span class="truncate font-mono text-[9px] text-neutral-600">
						{[extension(data.filePath), sampleRate > 0 && formatHz(sampleRate), sampleRate > 0 && channelLabel].filter(Boolean).join(' · ')}
					</span>
				{/if}
			</div>
			<Tooltip text="Choose file">
				<Button size="sm" icon class="nodrag nopan" type="button" onclick={chooseFile}>
					<Folder class="size-3.5" />
				</Button>
			</Tooltip>
			<Tooltip text="Start playback automatically when the pipeline runs">
				<Button size="sm" icon class="nodrag nopan" active={data.autoStart ?? true} onclick={toggleAutoStart}>
					<Autoplay class="size-3.5" />
				</Button>
			</Tooltip>
		</div>

		<!-- Scrubber: same track as Slider, the played part filled. -->
		<div
			bind:this={scrubEl}
			class={['nodrag nopan nowheel relative h-4 touch-none select-none', canScrub ? 'cursor-pointer' : 'opacity-40']}
			onpointerdown={scrubDown}
			onpointermove={scrubMove}
			onpointerup={scrubUp}
			onpointercancel={scrubUp}
			role="slider"
			tabindex="0"
			aria-label="Position"
			aria-valuemin={0}
			aria-valuemax={Math.max(totalFrames, 1)}
			aria-valuenow={scrubValue ?? frames}>
			<div class="absolute inset-x-[7px] inset-y-0">
				<div class="absolute inset-x-[-7px] top-1/2 h-1 -translate-y-1/2 rounded-full bg-(--lv-3)"></div>
				<div class="absolute top-1/2 left-[-7px] h-1 -translate-y-1/2 rounded-full bg-neutral-900" style="width: calc({playedPct}% + 7px);"></div>
				<div
					class="absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full border border-neutral-400 bg-white"
					style="left: {playedPct}%;">
				</div>
				{#if scrubValue !== null && sampleRate > 0}
					<span
						class="pointer-events-none absolute bottom-full z-[60] mb-1.5 -translate-x-1/2 rounded-md border border-theme/5 bg-(--lv-2) px-1.5 py-0.5 font-mono text-[10px] whitespace-nowrap text-neutral-1000 tabular-nums"
						style="left: {playedPct}%;">
						{formatDuration(scrubValue / sampleRate, 1)}
					</span>
				{/if}
			</div>
		</div>

		<div class="flex items-center justify-between font-mono text-[11px]">
			<span class="text-neutral-900 tabular-nums">{formatDuration(currentSec, 1)}</span>
			<div class="flex items-center justify-center gap-1">
				<span class="relative flex">
					<Tooltip text="Stop and rewind to start">
						<Button size="sm" icon class="nodrag nopan" type="button" disabled={!canControl} onclick={stop}>
							<Stop class="size-3" />
						</Button>
					</Tooltip>
					<ActionSocket nodeId={id} action="stop" onTrigger={stop} />
				</span>
				<span class="relative flex">
					<Tooltip text="Skip back 5 seconds">
						<Button size="sm" icon class="nodrag nopan" type="button" disabled={!canControl} onclick={skipBack}>
							<SkipBack5 class="size-3" />
						</Button>
					</Tooltip>
					<ActionSocket nodeId={id} action="back" onTrigger={skipBack} />
				</span>
				<span class="relative flex">
					<Tooltip text={playing ? 'Pause playback' : 'Play'}>
						<Button size="sm" icon class="nodrag nopan" type="button" disabled={!canControl} onclick={togglePlayPause}>
							{#if playing}
								<Pause class="size-3" />
							{:else}
								<Play class="size-3" />
							{/if}
						</Button>
					</Tooltip>
					<ActionSocket nodeId={id} action="play" onTrigger={togglePlayPause} />
				</span>
				<span class="relative flex">
					<Tooltip text="Skip forward 5 seconds">
						<Button size="sm" icon class="nodrag nopan" type="button" disabled={!canControl} onclick={skipForward}>
							<SkipForward5 class="size-3" />
						</Button>
					</Tooltip>
					<ActionSocket nodeId={id} action="forward" onTrigger={skipForward} />
				</span>
				<span class="relative flex">
					<Tooltip text={data.loopEnabled ? 'Looping on: restart at end' : 'Loop off: stop at end'}>
						<Button size="sm" icon class="nodrag nopan" active={data.loopEnabled} onclick={toggleLoop}>
							<Loop class="size-3" />
						</Button>
					</Tooltip>
					<ActionSocket nodeId={id} action="loop" onTrigger={toggleLoop} />
				</span>
			</div>
			<span class="text-neutral-900 tabular-nums">{formatDuration(totalSec, 1)}</span>
		</div>

		<Slider
			class="nodrag nopan nowheel"
			label="Volume"
			info="Playback level of the file before it enters the pipeline. 100% is the file as recorded."
			value={volumePct}
			min={0}
			max={100}
			step={1}
			format={formatPct}
			defaultValue={100}
			ticks={[25, 50, 75]}
			onChange={setVolume} />
	</div>
</Wrapper>
