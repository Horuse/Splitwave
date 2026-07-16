<script lang="ts">
	import { Handle, Position } from '@xyflow/svelte';
	import { channelColor, channelLabel } from '$lib/modules/flow/utils';

	interface Props {
		count: number;
		side: 'source' | 'target';
	}
	let { count, side }: Props = $props();

	let indices = $derived(Array.from({ length: count }, (_, i) => i));
</script>

<div class="flex flex-col gap-1">
	{#each indices as i (i)}
		{#if side === 'source'}
			<div class="relative -mr-4 flex min-h-4 items-center justify-end gap-1 pr-4">
				<span class="font-mono text-[9px] text-neutral-500">{channelLabel(i, count)}</span>
				<Handle
					type="source"
					id={`ch${i + 1}`}
					position={Position.Right}
					class="handle"
					style={`background:${channelColor(i)}`}
				/>
			</div>
		{:else}
			<div class="relative -ml-4 flex min-h-4 items-center gap-1 pl-4">
				<Handle
					type="target"
					id={`ch${i + 1}`}
					position={Position.Left}
					class="handle"
					style={`background:${channelColor(i)}`}
				/>
				<span class="font-mono text-[9px] text-neutral-500">{channelLabel(i, count)}</span>
			</div>
		{/if}
	{/each}
</div>
