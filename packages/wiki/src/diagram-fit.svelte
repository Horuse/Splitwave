<script lang="ts">
	import { useNodesInitialized, useSvelteFlow } from '@xyflow/svelte';
	import ZoomIn from '~icons/fluent/zoom-in-16-regular';
	import ZoomOut from '~icons/fluent/zoom-out-16-regular';
	import ZoomFit from '~icons/fluent/arrow-maximize-16-regular';

	// fitView on the canvas runs before nodes are measured; fit again once they are.
	const flow = useSvelteFlow();
	const initialized = useNodesInitialized();

	const FIT = { padding: 0.08, maxZoom: 1, duration: 0 };

	$effect(() => {
		if (initialized.current) requestAnimationFrame(() => flow.fitView(FIT));
	});

	const BUTTON = 'flex size-7 items-center justify-center text-neutral-900 hover:bg-(--lv-2) hover:text-neutral-1100';
</script>

<!-- Wheel zoom stays off so the article scrolls; these zoom the canvas instead. -->
<div class="nodrag nopan absolute right-2 bottom-2 z-10 flex overflow-hidden rounded-lg border border-theme/5 bg-neutral-200">
	<button type="button" class={BUTTON} title="Zoom out" aria-label="Zoom out" onclick={() => flow.zoomOut()}><ZoomOut class="size-4" /></button>
	<button type="button" class={BUTTON} title="Zoom in" aria-label="Zoom in" onclick={() => flow.zoomIn()}><ZoomIn class="size-4" /></button>
	<button type="button" class={BUTTON} title="Fit to view" aria-label="Fit to view" onclick={() => flow.fitView(FIT)}><ZoomFit class="size-4" /></button>
</div>
