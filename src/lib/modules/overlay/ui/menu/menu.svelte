<script lang="ts">
	import type { Snippet } from 'svelte';
	let { children, class: cls = '' }: { children: Snippet; class?: string } = $props();
</script>

<!-- svelte-ignore a11y_interactive_supports_focus a11y_click_events_have_key_events -->
<div
	class="min-w-44 overflow-hidden rounded-lg border border-neutral-400 bg-neutral-100 py-1 shadow-lg {cls}"
	role="menu"
	onclick={(e) => e.stopPropagation()}
>
	{@render children()}
</div>
