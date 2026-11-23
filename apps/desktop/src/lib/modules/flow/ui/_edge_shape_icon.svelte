<script lang="ts">
	import type { EdgeShape } from '../edge_settings.svelte';

	let { shape, animated = false }: { shape: EdgeShape; animated?: boolean } = $props();

	// One start and end for every shape: the picker reads as one cable rerouted.
	const PATHS: Record<EdgeShape, string> = {
		bezier: 'M6 7 C24 7 24 21 42 21',
		smoothstep: 'M6 7 H18 Q24 7 24 13 V15 Q24 21 30 21 H42',
		step: 'M6 7 H24 V21 H42',
		straight: 'M6 7 L42 21'
	};
</script>

<svg viewBox="0 0 48 28" class="h-7 w-12" fill="none" aria-hidden="true">
	<path
		d={PATHS[shape]}
		stroke="currentColor"
		stroke-width="2"
		stroke-linecap="round"
		stroke-linejoin="round"
		class={animated ? 'edge-icon-dash' : undefined} />
	<rect x="4" y="4" width="3" height="6" rx="1" fill="currentColor" />
	<rect x="41" y="18" width="3" height="6" rx="1" fill="currentColor" />
</svg>
