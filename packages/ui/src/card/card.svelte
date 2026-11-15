<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import { smoothRadius } from '../attachments';

	let { dashed = false, class: className, children }: { dashed?: boolean; class?: ClassValue; children?: Snippet } = $props();
</script>

<div data-layer="fg" class={['card', dashed && 'card-dashed', className]} {@attach smoothRadius({ variable: '--smooth-r' })}>
	{@render children?.()}
</div>
