<script lang="ts">
	import { getContext } from 'svelte';
	import { NodeResizer, type Node, type NodeProps } from '@xyflow/svelte';
	import type { WaveformNodeData } from '$lib/modules/pipeline/types';
	import { Pulse } from '$lib/components/icons';
	import { PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { CATEGORY_TEXT } from '$lib/modules/flow/utils/accents';
	import ChannelHandles from '../_channel_handles.svelte';
	import WaveformScope from '$lib/components/waveform_scope.svelte';

	const isPreview = getContext(PREVIEW_CTX) === true;

	type WaveformNodeType = Node<WaveformNodeData, 'waveform'>;
	let { id }: NodeProps<WaveformNodeType> = $props();
</script>

<div class={['flex flex-col rounded-2xl border border-neutral-400 bg-neutral-200 shadow-sm', isPreview ? 'h-40 w-80' : 'h-full w-full']}>
	{#if !isPreview}
		<NodeResizer minWidth={160} maxWidth={1200} minHeight={80} maxHeight={1200} />
	{/if}

	<div class="flex shrink-0 items-center justify-between px-3 pt-2 pb-1">
		<span class="flex items-center gap-1.5 text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">
			<Pulse class={['size-3 shrink-0', CATEGORY_TEXT.monitor]} />
			Waveform
		</span>
	</div>

	<div class="flex min-h-0 flex-1 items-start px-4 pb-2">
		{#if !isPreview}
			<ChannelHandles nodeId={id} side="target" />
		{/if}
		<div class="nowheel min-w-0 flex-1 self-stretch overflow-hidden">
			<WaveformScope nodeId={id} fill pan={false} />
		</div>
		{#if !isPreview}
			<ChannelHandles nodeId={id} side="source" />
		{/if}
	</div>
</div>
