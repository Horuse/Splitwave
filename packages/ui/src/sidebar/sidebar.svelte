<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import { ScrollArea } from '../scroll-area';

	let {
		header,
		footer,
		children,
		layer = 'fg',
		class: className
	}: {
		header?: Snippet;
		footer?: Snippet;
		children?: Snippet;
		/** `page` sits on the page background, so controls inside keep page shades. */
		layer?: 'fg' | 'page';
		class?: ClassValue;
	} = $props();
</script>

<aside data-layer={layer === 'fg' ? 'fg' : undefined} class={['sidebar', layer === 'page' && 'bg-background', className]}>
	{#if header}
		<div class="flex flex-col px-2 pt-2">
			{@render header()}
		</div>
	{/if}
	<ScrollArea class="flex-1" viewportClass="flex flex-col gap-0.5 p-2">
		<nav class="contents">
			{@render children?.()}
		</nav>
	</ScrollArea>
	<!-- Full-bleed rule, same tone as the header's bottom border. -->
	{#if footer}
		<div class="flex flex-col gap-0.5 border-t border-theme/5 p-2">
			{@render footer()}
		</div>
	{/if}
</aside>
