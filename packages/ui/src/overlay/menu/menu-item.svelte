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
		'flex h-8 w-full shrink-0 items-center gap-2.5 px-2.5 text-left text-sm transition-colors duration-100',
		danger ? 'text-red-700 dark:text-red-300' : 'text-neutral-1100',
		disabled ? 'cursor-not-allowed opacity-40' : danger ? 'hover:bg-red-500/15' : 'hover:bg-(--lv-3)'
	]}
	style="border-radius: var(--menu-item-r, 11.6px)"
	{disabled}
	{onclick}
	role="menuitem">
	{#if icon}
		{@const Ico = icon}
		<Ico class="size-4 shrink-0" />
	{/if}
	<span class="flex-1 truncate">
		{#if children}
			{@render children()}
		{:else}
			{label}
		{/if}
	</span>
	{#if shortcut}
		<span class="flex items-center gap-0.5 text-xs text-neutral-900">
			{@render shortcut()}
		</span>
	{/if}
</button>
