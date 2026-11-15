<script lang="ts">
	import ChannelHandles from '../_channel_handles.svelte';

	interface Props {
		nodeId: string;
		branch: number;
		label: string;
		open: boolean;
		live: boolean;
	}
	let { nodeId, branch, label, open, live }: Props = $props();
</script>

<div class="flex items-start gap-2">
	<span class={['flex min-h-4 items-center font-mono text-[9px]', open ? 'text-yellow-600 dark:text-yellow-400' : 'text-neutral-500']}>{label}</span>
	{#if live}
		<ChannelHandles {nodeId} side="source" {branch} labelled />
	{/if}
</div>
