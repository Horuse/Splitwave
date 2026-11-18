<script lang="ts">
	import Icon from './_icon.svelte';
	import type { ClassValue } from 'svelte/elements';
	let { class: cls = 'h-4 w-4', title }: { class?: ClassValue; title?: string } = $props();

	const svg = `<svg viewBox="0 0 16 16" xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path fill="none" d="M2 4.5 H14" stroke-dasharray="1.5 1.5" opacity=".55" /><path fill="none" d="M2 12.5 L4.5 12.5 L5.5 4.5 L10.5 4.5 L11.5 12.5 L14 12.5" /></svg>`;
</script>

<Icon {svg} class={cls} {title} />
