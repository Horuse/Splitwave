<script lang="ts">
	import { Panel, useSvelteFlow } from '@xyflow/svelte';
	import { Button } from '@splitwave/ui';
	import ZoomIn from '~icons/fluent/zoom-in-16-regular';
	import ZoomOut from '~icons/fluent/zoom-out-16-regular';
	import ZoomFit from '~icons/fluent/arrow-maximize-16-regular';

	const flow = useSvelteFlow();
</script>

<Panel position="bottom-left" class="flex flex-col gap-1">
	<Button icon type="button" title="Zoom in" aria-label="Zoom in" onclick={() => flow.zoomIn()}>
		<ZoomIn class="h-4 w-4" />
	</Button>
	<Button icon type="button" title="Zoom out" aria-label="Zoom out" onclick={() => flow.zoomOut()}>
		<ZoomOut class="h-4 w-4" />
	</Button>
	<Button icon type="button" title="Fit view" aria-label="Fit view" onclick={() => flow.fitView()}>
		<ZoomFit class="h-4 w-4" />
	</Button>
</Panel>
