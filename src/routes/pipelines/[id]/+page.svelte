<script lang="ts">
	import { page } from '$app/state';
	import type { Pipeline } from '$lib/modules/pipeline/types';
	import { methods as pipelineMethods } from '$lib/modules/pipeline/methods';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { ActivationButton, RunningTimer } from '$lib/modules/audio/ui';
	import Header from '$lib/components/layout/header.svelte';
	import Flow from '$lib/modules/flow';
	import { SnapshotHistory, SavedIndicator, UndoRedo } from '$lib/modules/flow/ui';
	import { Toaster } from 'svelte-french-toast';

	let pipeline = $state<Pipeline | null>(null);
	let notFound = $state(false);

	$effect(() => {
		const id = page.params.id;
		if (!id) {
			notFound = true;
			return;
		}
		(async () => {
			const p = await pipelineMethods.get(id);
			if (!p) {
				notFound = true;
			} else {
				pipeline = p;
			}
		})();
	});

	let nameSaveTimer: ReturnType<typeof setTimeout> | undefined;
	function onNameInput() {
		clearTimeout(nameSaveTimer);
		nameSaveTimer = setTimeout(() => {
			if (!pipeline) return;
			// Merge with the latest editor snapshot when one exists so we don't
			// clobber unsaved node/edge changes; otherwise persist the current
			// pipeline object with the new name.
			const snapshot = pipelineStore.editorActions?.getSnapshot();
			const next = snapshot
				? { ...snapshot, name: pipeline.name }
				: { ...pipeline, updatedAt: Date.now() };
			pipelineStore.save(next);
		}, 500);
	}
</script>

<Toaster position="bottom-end" containerClassName="mr-72" toastOptions={{
	duration: 5000,
	className: 'bg-neutral-200! rounded-xl! text-neutral-900! px-3!',
}} />

<Header>
	{#snippet left()}
		<div class="flex items-center gap-3">
			<a href="/" class="button-header px-4 text-sm">← Back</a>
			{#if pipeline}
				<input bind:value={pipeline.name} oninput={onNameInput} class="input-base" />
			{/if}
		</div>
	{/snippet}

	{#snippet right()}
		<div class="flex items-center gap-3">
			{#if audioStore.isRunning}
				<RunningTimer />
			{/if}
			{#if pipeline}
				<UndoRedo />
				<SavedIndicator />
				<SnapshotHistory pipelineId={pipeline.id} />
				<ActivationButton pipelineId={pipeline.id} />
			{/if}
		</div>
	{/snippet}
</Header>

<div class="flex h-[calc(100vh-40px)] w-full">
	{#if notFound}
		<div class="p-8 text-sm text-gray-500">Pipeline not found.</div>
	{:else if pipeline}
		<Flow.ui.Flow {pipeline} />
	{:else}
		<div class="p-8 text-sm text-gray-500">Loading…</div>
	{/if}
</div>
