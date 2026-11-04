<script lang="ts">
	import { Handle, Position, type Node, type NodeProps } from '@xyflow/svelte';
	import { controlEdgeStyle, handleColor, handleEdgeStyle, signalOf } from '$lib/modules/flow/utils';

	type SocketsNode = Node<{ label: string; side: 'source' | 'target'; handles: string[] }, 'sockets'>;
	let { data }: NodeProps<SocketsNode> = $props();

	let isSource = $derived(data.side === 'source');
</script>

<div class="rounded-2xl border border-dashed border-neutral-500 bg-neutral-100 p-4 text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">
	{data.label}
	<div class="mt-2 flex flex-col gap-2">
		{#each data.handles as h (h)}
			<div class={['relative flex h-3 items-center font-mono text-[9px] text-neutral-700 normal-case', isSource && 'justify-end']}>
				{h}
				<Handle
					type={data.side}
					id={h}
					position={isSource ? Position.Right : Position.Left}
					class="handle"
					style={signalOf(h) === 'control' ? controlEdgeStyle(data.side) : handleEdgeStyle(handleColor(h), data.side)} />
			</div>
		{/each}
	</div>
</div>
