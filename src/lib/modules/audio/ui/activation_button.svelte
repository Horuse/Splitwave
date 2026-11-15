<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import { Button } from '@splitwave/ui';
	import { audioStore } from '../stores.svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import Play from '~icons/fluent/play-24-filled';
	import Stop from '~icons/fluent/stop-24-filled';

	let { pipelineId, icon = false, size, class: className }: { pipelineId: string; icon?: boolean; size?: 'md' | 'lg' | 'xl'; class?: ClassValue } = $props();

	let label = $derived(audioStore.isRunning ? 'Stop' : 'Run');

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

<Button
	{icon}
	{size}
	class={className}
	tone={audioStore.isRunning ? 'danger' : 'brand'}
	loading={busy}
	loadingPosition={icon ? undefined : 'start'}
	title={label}
	aria-label={label}
	onclick={toggle}>
	{#if audioStore.isRunning}
		<Stop class="size-3.5" />
	{:else}
		<Play class="size-3.5" />
	{/if}
	{#if !icon}{label}{/if}
</Button>
