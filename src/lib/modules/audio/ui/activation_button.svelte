<script lang="ts">
	import { Button } from '@splitwave/ui';
	import { audioStore } from '../stores.svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import Play from '~icons/fluent/play-24-filled';
	import Stop from '~icons/fluent/stop-24-filled';

	let { pipelineId }: { pipelineId: string } = $props();

	let busy = $state(false);

	async function toggle() {
		if (busy) return;
		busy = true;
		try {
			if (audioStore.isRunning) {
				await audioStore.deactivatePipeline();
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

<Button tone={audioStore.isRunning ? 'danger' : 'brand'} loading={busy} loadingPosition="start" onclick={toggle}>
	{#if audioStore.isRunning}
		<Stop class="size-3.5" />
		Stop
	{:else}
		<Play class="size-3.5" />
		Run
	{/if}
</Button>
