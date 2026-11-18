<script lang="ts">
	import Icon from './_icon.svelte';
	import type { ClassValue } from 'svelte/elements';
	let { class: cls = 'h-4 w-4', title }: { class?: ClassValue; title?: string } = $props();

	const svg = `<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path fill="none" d="M2 13.5 L7 8.5 C8.5 7 10 6.5 14 6" /><path fill="none" d="M7 2.5 V13.5" stroke-dasharray="1.5 1.5" opacity=".55" /></svg>`;
</script>

<Icon {svg} class={cls} {title} />
