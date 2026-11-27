<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import Default from '~icons/fluent/sound-wave-circle-20-filled';
	import { iconStore } from '../icons/store.svelte';
	import IconGlyph from './icon_glyph.svelte';

	let { name, class: className }: { name?: string; class?: ClassValue } = $props();

	$effect(() => {
		if (name) void iconStore.load();
	});

	const svg = $derived(name ? iconStore.svg(name) : null);
</script>

{#if svg}
	<IconGlyph {svg} class={className} />
{:else}
	<Default class={className} />
{/if}
