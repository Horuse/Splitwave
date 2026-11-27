<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import { audioStore } from '../stores.svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import RunButton, { type RunState } from './run_button.svelte';

	let {
		pipelineId,
		size,
		opaque = false,
		class: className
	}: {
		pipelineId: string;
		size?: 'md' | 'lg' | 'xl';
		/** Same tint mixed into the background, for a button that sits over the canvas. */
		opaque?: boolean;
		class?: ClassValue;
	} = $props();

	const OPAQUE: Record<RunState, string> = {
		stop: 'bg-[color-mix(in_oklab,var(--color-red-500)_40%,var(--color-background))] hover:bg-[color-mix(in_oklab,var(--color-red-500)_50%,var(--color-background))]',
		run: 'bg-[color-mix(in_oklab,var(--color-orange-500)_40%,var(--color-background))] hover:bg-[color-mix(in_oklab,var(--color-orange-500)_50%,var(--color-background))]',
		switch: 'bg-[color-mix(in_oklab,var(--color-amber-500)_40%,var(--color-background))] hover:bg-[color-mix(in_oklab,var(--color-amber-500)_50%,var(--color-background))]'
	};

	let runState = $derived<RunState>(audioStore.isRunningPipeline(pipelineId) ? 'stop' : audioStore.isRunning ? 'switch' : 'run');

	let busy = $state(false);

	async function toggle() {
		if (busy) return;
		busy = true;
		try {
			if (audioStore.isRunningPipeline(pipelineId)) {
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

<RunButton state={runState} {size} loading={busy} class={[opaque && OPAQUE[runState], className]} onclick={toggle} />
