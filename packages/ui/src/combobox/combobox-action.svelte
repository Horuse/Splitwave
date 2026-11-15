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

<button type="button" class="combobox-action" {onclick}>
	{#if Icon}
		<Icon class="size-3.5 shrink-0 {iconClass}" />
	{/if}
	<span class="truncate">{label}</span>
</button>
