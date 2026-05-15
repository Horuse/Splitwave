<script lang="ts">
	import type { Component, Snippet } from 'svelte';

	interface Props {
		label?: string;
		icon?: Component<{ class?: string }>;
		shortcut?: Snippet;
		danger?: boolean;
		disabled?: boolean;
		onclick?: () => void;
		children?: Snippet;
	}

	let { label, icon, shortcut, danger = false, disabled = false, onclick, children }: Props = $props();
</script>

<button
	type="button"
	class={[
		'flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm',
		danger ? 'text-red-700 dark:text-red-300' : 'text-neutral-1100',
		disabled
			? 'cursor-not-allowed opacity-40'
			: danger
				? 'hover:bg-red-500/15'
				: 'hover:bg-neutral-200'
	]}
	{disabled}
	{onclick}
	role="menuitem"
>
	{#if icon}
		{@const Ico = icon}
		<Ico class="h-3.5 w-3.5 shrink-0 opacity-70" />
	{:else}
		<span class="h-3.5 w-3.5 shrink-0"></span>
	{/if}
	<span class="flex-1">
		{#if children}
			{@render children()}
		{:else}
			{label}
		{/if}
	</span>
	{#if shortcut}
		<span class="flex items-center gap-0.5 font-mono text-[10px] text-neutral-900">
			{@render shortcut()}
		</span>
	{/if}
</button>
