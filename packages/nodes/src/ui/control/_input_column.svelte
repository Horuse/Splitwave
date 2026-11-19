<script lang="ts">
	import ChannelHandles from '../_channel_handles.svelte';

	let { nodeId, live }: { nodeId: string; live: boolean } = $props();
</script>

<div class="flex flex-col gap-0.5">
	<span class="font-mono text-[9px] text-neutral-500">in</span>
	{#if live}
		<ChannelHandles {nodeId} side="target" labelled />
	{/if}
</div>
