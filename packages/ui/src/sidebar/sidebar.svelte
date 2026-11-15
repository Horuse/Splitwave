<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';

	let { header, footer, children, class: className }: { header?: Snippet; footer?: Snippet; children?: Snippet; class?: ClassValue } = $props();
</script>

<aside data-layer="fg" class={['sidebar', className]}>
	{@render header?.()}
	<nav class="flex flex-col gap-0.5">
		{@render children?.()}
	</nav>
	{#if footer}
		<div class="mt-auto flex flex-col gap-0.5">
			{@render footer()}
		</div>
	{/if}
</aside>
