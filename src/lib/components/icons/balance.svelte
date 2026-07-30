<script lang="ts">
	import Icon from './_icon.svelte';
	import type { ClassValue } from 'svelte/elements';
	let { class: cls = 'h-4 w-4', title }: { class?: ClassValue; title?: string } = $props();

	const svg = `<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path fill="none" d="M2 5.5 H14" opacity=".5" /><path fill="none" d="M2 10.5 H14" opacity=".5" /><circle cx="5" cy="5.5" r="2" fill="currentColor" stroke="none" /><circle cx="11" cy="10.5" r="2" fill="currentColor" stroke="none" /></svg>`;
</script>

<Icon {svg} class={cls} {title} />
