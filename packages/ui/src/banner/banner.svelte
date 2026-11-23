<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import { smoothRadius } from '../attachments';

	let { class: className, children }: { class?: ClassValue; children?: Snippet } = $props();
</script>

<div class={['banner', className]} {@attach smoothRadius({ variable: '--smooth-r', base: 14 })}>
	{@render children?.()}
</div>
