<script lang="ts">
	import ChannelHandles from '../_channel_handles.svelte';

	let { nodeId, live }: { nodeId: string; live: boolean } = $props();
</script>

<div class="flex flex-col gap-0.5">
	<span class="text-[10px] text-neutral-700">in</span>
	{#if live}
		<ChannelHandles {nodeId} side="target" labelled />
	{/if}
</div>
