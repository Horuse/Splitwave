<script lang="ts">
	import { slide } from 'svelte/transition';
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';

	type Props = {
		tone?: 'warning' | 'info' | 'success' | 'danger';
		class?: ClassValue;
		children?: Snippet;
	};

	let { tone = 'warning', class: className, children }: Props = $props();
</script>

<div transition:slide={{ duration: 150 }} class={['callout', tone !== 'warning' && `callout-${tone}`, className]} role="note">
	{@render children?.()}
</div>
