<script lang="ts">
	import { Tooltip } from '@splitwave/ui';
	import { getContext, type Component, type Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import { Handle, Position } from '@xyflow/svelte';
	import { MAIN_HANDLE, PREVIEW_CTX, CATEGORY_TEXT } from './utils';
	import type { NodeCategory } from '@splitwave/nodes';
	import ArrowSwap from '~icons/fluent/arrow-swap-16-filled';
	import Hourglass from '~icons/fluent/hourglass-16-filled';
	import { getEnginePort } from '../engine';
	import { formatLatencyMs } from '@splitwave/nodes';
	import ChannelHandles from './_channel_handles.svelte';

	const isPreview = getContext(PREVIEW_CTX) === true;
	const engine = getEnginePort();

	export interface InputHandleConfig {
		id: string;
		label?: string;
		position?: 'left' | 'bottom' | 'top';
	}

	interface Props {
		label: string;
		accent?: NodeCategory;
		icon?: Component<{ class?: ClassValue; title?: string }>;
		badge?: Snippet;
		srcTooltip?: string;
		hasInput?: boolean;
		hasOutput?: boolean;
		inputs?: InputHandleConfig[];
		outputLabel?: string;
		bypassed?: boolean;
		onBypass?: () => void;
		// When set, the node exposes per-channel handles that grow with the cables
		// wired into it instead of a single bus handle. Requires `nodeId`.
		channelIo?: boolean;
		nodeId?: string;
		maxChannels?: number;
		minChannels?: number;
		/** Meters widen with every channel, so they opt out of the node width cap. */
		wide?: boolean;
		selfGrowing?: boolean;
		children?: Snippet;
	}

	let {
		label,
		accent = 'effect',
		icon: NodeIcon,
		badge,
		srcTooltip,
		hasInput = false,
		hasOutput = false,
		inputs,
		outputLabel,
		bypassed,
		onBypass,
		channelIo = false,
		nodeId,
		maxChannels,
		minChannels,
		wide = false,
		selfGrowing = false,
		children
	}: Props = $props();

	let chExpanded = $derived(channelIo && !!nodeId && !isPreview);

	// A node that cannot keep up with the engine buffer runs at a larger block
	// of its own; say so, the way the resampling icon flags a rate change.
	let timing = $derived(nodeId ? engine.nodeTiming(nodeId) : undefined);
	let blockTooltip = $derived.by(() => {
		const block = timing?.workingBlock;
		if (!block || engine.bufferFrames === null || !timing!.sampleRate) return undefined;
		return `Runs in ${block}-frame blocks (buffer ${engine.bufferFrames}), adding ${formatLatencyMs((timing!.latencyFrames * 1000) / timing!.sampleRate)} ms`;
	});

	function pos(p: InputHandleConfig['position']): Position {
		if (p === 'bottom') return Position.Bottom;
		if (p === 'top') return Position.Top;
		return Position.Left;
	}

	function labelClasses(p: InputHandleConfig['position']): string {
		const base = 'pointer-events-none absolute px-1 font-mono text-[9px] leading-none text-neutral-700';
		if (p === 'bottom') return `${base} whitespace-nowrap bottom-full mb-0.5 left-1/2 -translate-x-1/2`;
		if (p === 'top') return `${base} whitespace-nowrap top-full mt-0.5 left-1/2 -translate-x-1/2`;
		return `${base} left-full ml-0.5 top-1/2 -translate-y-1/2 [writing-mode:vertical-lr]`;
	}
</script>

<!-- Concentric corners: 6px controls + 16px padding = 22px. The padding also sets where channel sockets sit (edgeOffset). -->
<div data-layer="popover" class={['node-shell min-w-32 rounded-[22px] border border-theme/5 bg-neutral-200 p-4', !wide && 'max-w-80']}>
	<div class="relative mb-2 flex items-center justify-between gap-2">
		<span class="flex items-center gap-1.5 text-xs font-medium text-neutral-1000">
			{#if NodeIcon}
				<NodeIcon class={['size-3.5 shrink-0', CATEGORY_TEXT[accent]]} />
			{/if}
			{label}
			{#if srcTooltip}
				<Tooltip text={srcTooltip}>
					<span class="inline-flex cursor-help items-center text-amber-600 transition-colors hover:text-amber-700">
						<ArrowSwap class="size-3" />
					</span>
				</Tooltip>
			{/if}
			{#if blockTooltip}
				<Tooltip text={blockTooltip}>
					<span class="inline-flex cursor-help items-center text-amber-600 transition-colors hover:text-amber-700">
						<Hourglass class="size-3" />
					</span>
				</Tooltip>
			{/if}
		</span>
		<div class="flex items-center gap-1">
			{#if badge}{@render badge()}{/if}
			{#if onBypass}
				<button
					type="button"
					class={[
						'nodrag nopan flex h-5 items-center rounded-md px-1.5 text-[10px] font-medium',
						bypassed ? 'bg-amber-500/15 text-amber-600 hover:bg-amber-500/25' : 'bg-(--lv-2) text-neutral-900 hover:bg-(--lv-3)'
					]}
					title={bypassed ? 'Bypassed -- click to engage' : 'Engaged -- click to bypass'}
					onclick={onBypass}>
					{bypassed ? 'Bypassed' : 'On'}
				</button>
			{/if}
		</div>
	</div>

	<!-- Socket columns take no width; a gap here would inset the content. -->
	<div class="flex items-start">
		{#if chExpanded && nodeId && hasInput}
			<ChannelHandles {nodeId} side="target" max={maxChannels} min={minChannels} />
		{/if}
		<div class={['min-w-0 flex-1', bypassed && 'opacity-40']}>
			{@render children?.()}
		</div>
		{#if chExpanded && nodeId && hasOutput}
			<ChannelHandles {nodeId} side="source" max={maxChannels} min={minChannels} {selfGrowing} />
		{/if}
	</div>

	<!-- Named inputs (a sidechain) sit outside the channel columns, so they
	     survive alongside them. -->
	{#if !isPreview && inputs}
		{#each inputs as h (h.id)}
			<Handle type="target" id={h.id} class="handle" position={pos(h.position)}>
				{#if h.label}
					<span class={labelClasses(h.position)}>{h.label}</span>
				{/if}
			</Handle>
		{/each}
	{/if}

	{#if !isPreview && !chExpanded}
		{#if !inputs && hasInput}
			<Handle type="target" id={MAIN_HANDLE} class="handle" position={Position.Left} />
		{/if}
		{#if hasOutput}
			<Handle type="source" id={MAIN_HANDLE} class="handle" position={Position.Right}>
				{#if outputLabel}
					<span
						class="pointer-events-none absolute top-1/2 right-full mr-0.5 -translate-y-1/2 px-1 font-mono text-[9px] leading-none text-neutral-700 [writing-mode:vertical-rl]">
						{outputLabel}
					</span>
				{/if}
			</Handle>
		{/if}
	{/if}
</div>

<style>
	/* XYFlow marks selection on the node wrapper it owns, one level above us. */
	:global(.svelte-flow__node.selected) .node-shell {
		border-color: var(--color-neutral-700);
	}
</style>
