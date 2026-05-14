<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	interface Props {
		svg: string;
		class?: ClassValue;
		title?: string;
	}
	let { svg, class: cls = 'h-4 w-4', title }: Props = $props();
</script>

<span class={cls} aria-hidden={title ? undefined : true} aria-label={title} role={title ? 'img' : undefined}
>{@html svg}</span>

<style>
	span :global(svg) {
		width: 100%;
		height: 100%;
		fill: currentColor;
		display: block;
	}
</style>
