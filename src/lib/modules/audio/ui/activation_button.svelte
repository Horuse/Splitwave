<script lang="ts">
	import { methods } from '../methods';
	import { audioStore } from '../stores.svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';

	let { pipelineId }: { pipelineId: string } = $props();

	let busy = $state(false);

	async function toggle() {
		if (busy) return;
		busy = true;
		try {
			if (audioStore.isRunning) {
				await methods.stopPipeline();
			} else {
				const snapshot = pipelineStore.editorActions?.getSnapshot();
				if (!snapshot) {
					audioStore.reportError('No pipeline loaded');
					return;
				}
				await audioStore.activatePipeline(pipelineId, { nodes: snapshot.nodes, edges: snapshot.edges });
			}
		} catch (e) {
			audioStore.reportError(e);
		} finally {
			busy = false;
		}
	}
</script>

<button
	class={[
		'button-main primary px-4 py-0 rounded-lg',
		!audioStore.isRunning && 'green',
		audioStore.isRunning && 'red'
	]}
	disabled={busy}
	onclick={toggle}
>
	{#if busy}
		…
	{:else if audioStore.isRunning}
		Stop
	{:else}
		Activate
	{/if}
</button>
