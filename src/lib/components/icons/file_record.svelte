<script lang="ts">
	import Icon from './_icon.svelte';
	import type { ClassValue } from 'svelte/elements';
	let { class: cls = 'h-4 w-4', title }: { class?: ClassValue; title?: string } = $props();

	const svg = `<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path fill="none" d="M9 1.75 H4.25 A1.5 1.5 0 0 0 2.75 3.25 V12.75 A1.5 1.5 0 0 0 4.25 14.25 H11.75 A1.5 1.5 0 0 0 13.25 12.75 V6 Z" /><path fill="none" d="M8.75 1.75 V5.5 A0.75 0.75 0 0 0 9.5 6.25 H13.25" /><circle cx="8" cy="9.75" r="2.25" fill="currentColor" stroke="none" /></svg>`;
</script>

<Icon {svg} class={cls} {title} />
