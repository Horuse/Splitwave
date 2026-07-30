<script lang="ts">
	import Icon from './_icon.svelte';
	import type { ClassValue } from 'svelte/elements';
	let { class: cls = 'h-4 w-4', title }: { class?: ClassValue; title?: string } = $props();

	const svg = `<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path fill="none" d="M2 14 H14" opacity=".55" /><path fill="none" d="M3.25 3.5 V13" /><path fill="none" d="M6.75 6.5 V13" opacity=".8" /><path fill="none" d="M10.25 9 V13" opacity=".6" /><path fill="none" d="M13.75 11 V13" opacity=".4" /></svg>`;
</script>

<Icon {svg} class={cls} {title} />
