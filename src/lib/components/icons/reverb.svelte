<script lang="ts">
	import Icon from './_icon.svelte';
	import type { ClassValue } from 'svelte/elements';
	let { class: cls = 'h-4 w-4', title }: { class?: ClassValue; title?: string } = $props();

	const svg = `<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path fill="none" d="M3 4.5 V11.5" /><path fill="none" d="M6.5 5 A4.5 4.5 0 0 1 6.5 11" opacity=".8" /><path fill="none" d="M9.5 3.5 A7 7 0 0 1 9.5 12.5" opacity=".6" /><path fill="none" d="M12.5 2 A9.5 9.5 0 0 1 12.5 14" opacity=".4" /></svg>`;
</script>

<Icon {svg} class={cls} {title} />
