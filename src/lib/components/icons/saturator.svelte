<script lang="ts">
	import Icon from './_icon.svelte';
	import type { ClassValue } from 'svelte/elements';
	let { class: cls = 'h-4 w-4', title }: { class?: ClassValue; title?: string } = $props();

	const svg = `<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path fill="none" d="M2 13.5 C5.5 13.5 4.5 8 8 8 C11.5 8 10.5 2.5 14 2.5" /></svg>`;
</script>

<Icon {svg} class={cls} {title} />
