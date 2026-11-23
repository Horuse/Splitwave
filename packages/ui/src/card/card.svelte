<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import { smoothRadius } from '../attachments';

	let {
		dashed = false,
		nested = false,
		class: className,
		children
	}: {
		dashed?: boolean;
		/** Concentric corners: radius = first child's radius + padding. */
		nested?: boolean;
		class?: ClassValue;
		children?: Snippet;
	} = $props();
</script>

<div data-layer="fg" class={['card', dashed && 'card-dashed', className]} {@attach smoothRadius({ variable: '--smooth-r', nested })}>
	{@render children?.()}
</div>
