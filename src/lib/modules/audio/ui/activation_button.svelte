<script lang="ts">
	import { audioStore } from '../stores.svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { t } from '$lib/modules/i18n';

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
					audioStore.reportError(t('audio.noPipelineLoaded'));
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

<button class={['button-main primary rounded-lg px-4 py-0', !audioStore.isRunning && 'green', audioStore.isRunning && 'red']} disabled={busy} onclick={toggle}>
	{#if busy}
		…
	{:else if audioStore.isRunning}
		{t('common.stop')}
	{:else}
		{t('common.activate')}
	{/if}
</button>
