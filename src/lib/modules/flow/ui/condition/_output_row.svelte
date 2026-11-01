<script lang="ts">
	import ChannelHandles from '../_channel_handles.svelte';

	interface Props {
		nodeId: string;
		branch: number;
		label: string;
		open: boolean;
		/** Null while a control input owns the choice. */
		onSelect: (() => void) | null;
		live: boolean;
	}
	let { nodeId, branch, label, open, onSelect, live }: Props = $props();
</script>

<div class="flex items-start justify-end gap-2">
	<button
		class={[
			'nodrag nopan min-w-12 rounded-md border px-2 py-0.5 font-mono text-[10px] transition-colors',
			open ? 'border-yellow-500/60 bg-yellow-500/10 text-yellow-600 dark:text-yellow-400' : 'border-neutral-400 bg-neutral-100 text-neutral-900',
			onSelect && !open && 'hover:bg-neutral-200'
		]}
		title={onSelect ? `Open ${label}` : 'Driven by the control input'}
		disabled={!onSelect}
		onclick={() => onSelect?.()}>
		{label}
	</button>
	{#if live}
		<ChannelHandles {nodeId} side="source" {branch} />
	{/if}
</div>
