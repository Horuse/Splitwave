<script lang="ts">
	import { Handle, Position, type Node, type NodeProps } from '@xyflow/svelte';
	import { channelColor, channelLabel, handleEdgeStyle } from '$lib/modules/flow/utils';

	type StubNode = Node<{ channels: number }, 'stub'>;
	let { data }: NodeProps<StubNode> = $props();
</script>

<div class="rounded-2xl border border-dashed border-neutral-500 bg-neutral-100 p-4 text-[10px] font-semibold tracking-wider text-neutral-900 uppercase">
	Source
	<div class="mt-2 flex flex-col gap-2">
		{#each { length: data.channels } as _, i (i)}
			<div class="relative flex h-3 items-center justify-end font-mono text-[9px] text-neutral-700 normal-case">
				{channelLabel(i, data.channels)}
				<Handle type="source" id={`ch${i + 1}`} position={Position.Right} class="handle" style={handleEdgeStyle(channelColor(i), 'source')} />
			</div>
		{/each}
	</div>
</div>
