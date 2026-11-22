<script lang="ts">
	import { useNodesInitialized, useSvelteFlow } from '@xyflow/svelte';
	import ZoomIn from '~icons/fluent/zoom-in-16-regular';
	import ZoomOut from '~icons/fluent/zoom-out-16-regular';
	import ZoomFit from '~icons/fluent/arrow-maximize-16-regular';
	import { Button } from '@splitwave/ui';

	// fitView on the canvas runs before nodes are measured; fit again once they are.
	const flow = useSvelteFlow();
	const initialized = useNodesInitialized();

	const FIT = { padding: 0.08, maxZoom: 1, duration: 0 };

	$effect(() => {
		if (initialized.current) requestAnimationFrame(() => flow.fitView(FIT));
	});

</script>

<!-- Wheel zoom stays off so the article scrolls; these zoom the canvas instead. -->
<div class="nodrag nopan absolute right-2 bottom-2 z-10 flex gap-1">
	<Button icon type="button" title="Zoom out" aria-label="Zoom out" onclick={() => flow.zoomOut()}><ZoomOut class="h-4 w-4" /></Button>
	<Button icon type="button" title="Zoom in" aria-label="Zoom in" onclick={() => flow.zoomIn()}><ZoomIn class="h-4 w-4" /></Button>
	<Button icon type="button" title="Fit to view" aria-label="Fit to view" onclick={() => flow.fitView(FIT)}><ZoomFit class="h-4 w-4" /></Button>
</div>
