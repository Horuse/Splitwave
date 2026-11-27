<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import type { IconSvg } from '../icons/store.svelte';

	let { svg, class: className }: { svg: IconSvg; class?: ClassValue } = $props();
</script>

<svg class={className} viewBox={svg.viewBox} aria-hidden="true">
	<!-- eslint-disable-next-line svelte/no-at-html-tags -->
	{@html svg.body}
</svg>
