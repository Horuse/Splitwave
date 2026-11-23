<script lang="ts">
	import { fly } from 'svelte/transition';
	import type { Component, Snippet } from 'svelte';
	import { smoothRadius } from '../attachments';

	type Props = {
		label: string;
		icon?: Component<{ class?: string }>;
		href?: string;
		active?: boolean;
		nested?: boolean;
		title?: string;
		onclick?: () => void;
		trailing?: Snippet;
	};

	let { label, icon: Icon, href, active = false, nested = false, title, onclick, trailing }: Props = $props();
	const radius = smoothRadius({ variable: '--r', maxRatio: 0.35 });
</script>

{#snippet content()}
	{#if active && !nested}
		<span class="sidebar-marker" transition:fly={{ x: -4, duration: 180 }}></span>
	{/if}
	{#if Icon}<Icon />{/if}
	<span class="flex-1 truncate">{label}</span>
	{@render trailing?.()}
{/snippet}

{#if href}
	<a
		class={['sidebar-item', nested && 'sidebar-item-nested']}
		{href}
		{title}
		data-active={active}
		aria-current={active ? 'page' : undefined}
		{@attach radius}>
		{@render content()}
	</a>
{:else}
	<button type="button" class={['sidebar-item', nested && 'sidebar-item-nested']} {title} data-active={active} {onclick} {@attach radius}>
		{@render content()}
	</button>
{/if}
