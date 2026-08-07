<script lang="ts">
	import { getContext, type Component, type Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import { Handle, Position } from '@xyflow/svelte';
	import { PREVIEW_CTX } from '../utils';
	import { CATEGORY_TEXT } from '../utils/accents';
	import type { NodeCategory } from '$lib/modules/pipeline/types';
	import ChannelHandles from './_channel_handles.svelte';

	const isPreview = getContext(PREVIEW_CTX) === true;

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

<div class={['node-shell min-w-32 rounded-2xl border border-neutral-400 bg-neutral-200 p-4 shadow-sm', !wide && 'max-w-80']}>
	<div class="mb-2 flex items-center justify-between gap-2">
		<span class="flex items-center gap-1.5 text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">
			{#if NodeIcon}
				<NodeIcon class={['size-3 shrink-0', CATEGORY_TEXT[accent]]} />
			{/if}
			{label}
		</span>
		<div class="flex items-center gap-1">
			{#if badge}{@render badge()}{/if}
			{#if onBypass}
				<button
					type="button"
					class={[
						'nodrag nopan flex h-4 items-center rounded border px-1.5 font-mono text-[9px] transition-colors',
						bypassed
							? 'border-amber-500 bg-amber-100 text-amber-900 hover:bg-amber-200'
							: 'border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
					]}
					title={bypassed ? 'Bypassed -- click to engage' : 'Engaged -- click to bypass'}
					onclick={onBypass}>
					{bypassed ? 'BYP' : 'ON'}
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
			<Handle type="target" class="handle" position={Position.Left} />
		{/if}
		{#if hasOutput}
			<Handle type="source" class="handle" position={Position.Right}>
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
