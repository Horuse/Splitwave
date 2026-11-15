<script lang="ts">
	import type { Component, Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import { smoothRadius } from '../attachments';
	import { Card } from '../card';

	type Props = {
		title: string;
		description?: string;
		icon?: Component<{ class?: string }>;
		class?: ClassValue;
		children?: Snippet;
	};

	let { title, description, icon: Icon, class: className, children }: Props = $props();
</script>

<Card class={['flex flex-col gap-5 p-6', className]}>
	<header class="flex items-start gap-3">
		{#if Icon}
			<span class="section-card-icon" {@attach smoothRadius({ variable: '--r' })}><Icon /></span>
		{/if}
		<div class="flex min-w-0 flex-col gap-0.5">
			<h2 class="text-sm font-semibold text-theme">{title}</h2>
			{#if description}<p class="text-xs text-neutral-900">{description}</p>{/if}
		</div>
	</header>
	{@render children?.()}
</Card>
