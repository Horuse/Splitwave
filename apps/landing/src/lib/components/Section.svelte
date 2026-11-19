<script lang="ts">
	import type { Snippet } from 'svelte';

	let { label, children }: { label: string; children: Snippet } = $props();
</script>

<section class="flex flex-col gap-5">
	<h2 class="text-base font-semibold">{label}</h2>
	{@render children()}
</section>
