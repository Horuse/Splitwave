<script lang="ts">
	import type { Snippet } from 'svelte';

	/** Puts an app store in the state a story shows before its children render. */
	let { apply, children }: { apply: () => void; children: Snippet } = $props();

	// svelte-ignore state_referenced_locally
	apply();
</script>

{@render children()}
