<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { ClassValue } from 'svelte/elements';
	import { smoothRadius } from '../../attachments';

	let { children, class: className }: { children: Snippet; class?: ClassValue } = $props();
</script>

<!-- Items are 32px tall, so their radius is fixed; the panel adds its padding and border to stay concentric. -->
<!-- svelte-ignore a11y_interactive_supports_focus a11y_click_events_have_key_events -->
<div
	data-layer="popover"
	class={['flex min-w-44 flex-col border border-neutral-400 bg-popover p-1 shadow-lg', className]}
	style="border-radius: calc(var(--menu-item-r, 11.6px) + 5px)"
	{@attach smoothRadius({ variable: '--menu-item-r', side: 32 })}
	role="menu"
	onclick={(e) => e.stopPropagation()}>
	{@render children()}
</div>
