<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import { NodeResizer, useNodeConnections, useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { WaveformNodeData } from '$lib/modules/pipeline/types';
	import { Pulse } from '$lib/components/icons';
	import { parseHandle, PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { CATEGORY_TEXT } from '$lib/modules/flow/utils/accents';
	import ChannelHandles from '../_channel_handles.svelte';
	import WaveformScope from '$lib/components/waveform_scope.svelte';

	const isPreview = getContext(PREVIEW_CTX) === true;

	type WaveformNodeType = Node<WaveformNodeData, 'waveform'>;
	let { id }: NodeProps<WaveformNodeType> = $props();

	const nodeId = untrack(() => id);
	const flow = useSvelteFlow();
	const TIME_SCALE_HEIGHT = 22;
	const incoming = useNodeConnections({ id: nodeId, handleType: 'target' });
	let displayedChannels = $derived(
		incoming.current.reduce((count, connection) => {
			const channel = connection.targetHandle ? parseHandle(connection.targetHandle) : null;
			return channel === null ? count : Math.max(count, channel);
		}, 1)
	);
	let canvasMinHeight = $derived(TIME_SCALE_HEIGHT + displayedChannels * 72);
	let nodeMinHeight = $derived(canvasMinHeight + 36);
	let previousMinHeight: number | undefined;

	$effect(() => {
		if (isPreview) return;
		const minHeight = nodeMinHeight;
		const node = untrack(() => flow.getNode(nodeId));
		if (!node) return;
		const width = Math.max(node.width ?? node.measured?.width ?? 0, 256);
		const currentHeight = node.height ?? node.measured?.height ?? 0;
		const height =
			previousMinHeight === undefined ? Math.max(currentHeight, minHeight) : Math.max(minHeight, currentHeight + minHeight - previousMinHeight);
		previousMinHeight = minHeight;
		if (width !== node.width || height !== node.height) flow.updateNode(nodeId, { width, height });
	});
</script>

<div
	class={['flex min-w-64 flex-col rounded-2xl border border-neutral-400 bg-neutral-200 shadow-sm', isPreview ? 'h-40 w-80' : 'h-full w-full']}
	style:min-height={`${nodeMinHeight}px`}>
	{#if !isPreview}
		<NodeResizer minWidth={256} maxWidth={1200} minHeight={nodeMinHeight} maxHeight={1200} />
	{/if}

	<div class="flex shrink-0 items-center justify-between px-3 pt-2 pb-1">
		<span class="flex items-center gap-1.5 text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">
			<Pulse class={['size-3 shrink-0', CATEGORY_TEXT.monitor]} />
			Waveform
		</span>
	</div>

	<div class="flex min-h-0 flex-1 items-start px-4 pb-2">
		{#if !isPreview}
			<ChannelHandles {nodeId} side="target" />
		{/if}
		<div class="nowheel min-w-0 flex-1 self-stretch overflow-hidden" style:min-height={`${canvasMinHeight}px`}>
			<WaveformScope {nodeId} fill pan={false} maxChannels={displayedChannels} />
		</div>
		{#if !isPreview}
			<ChannelHandles {nodeId} side="source" />
		{/if}
	</div>
</div>
