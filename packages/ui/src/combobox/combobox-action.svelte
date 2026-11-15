<script lang="ts">
	import type { Component } from 'svelte';

	interface Props {
		label: string;
		icon?: Component<{ class?: string }>;
		/** Extra classes for the icon, e.g. `animate-spin` while an action runs. */
		iconClass?: string;
		onclick: () => void;
	}

	let { label, icon: Icon, iconClass = '', onclick }: Props = $props();
</script>

<button
	type="button"
	class="nodrag nopan flex w-full items-center gap-1.5 border-t border-(--lv-3) bg-(--lv-1) px-2 py-1.5 text-left text-xs font-medium text-neutral-1000 transition-colors duration-150 hover:bg-(--lv-2)"
	{onclick}>
	{#if Icon}
		<Icon class="size-3.5 shrink-0 {iconClass}" />
	{/if}
	<span class="truncate">{label}</span>
</button>
