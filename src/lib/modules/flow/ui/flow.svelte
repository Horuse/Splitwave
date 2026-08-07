<script lang="ts">
	import { SvelteFlowProvider } from '@xyflow/svelte';
	import type { Pipeline } from '$lib/modules/pipeline/types';
	import Editor from './editor.svelte';
	import '@xyflow/svelte/dist/base.css';

	let { pipeline }: { pipeline: Pipeline } = $props();
</script>

<SvelteFlowProvider>
	<Editor {pipeline} />
</SvelteFlowProvider>
