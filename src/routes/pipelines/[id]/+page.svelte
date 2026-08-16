<script lang="ts">
	import { page } from '$app/state';
	import type { Pipeline } from '$lib/modules/pipeline/types';
	import { methods as pipelineMethods } from '$lib/modules/pipeline/methods';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { ActivationButton, LatencyBadge, RunningTimer } from '$lib/modules/audio/ui';
	import Header from '$lib/components/layout/header.svelte';
	import Flow from '$lib/modules/flow';
	import { SnapshotHistory, SavedIndicator, UndoRedo } from '$lib/modules/flow/ui';
	import { Toaster } from 'svelte-french-toast';
	import { isFromFuture } from '$lib/modules/pipeline/migrations';

	let pipeline = $state<Pipeline | null>(null);
	let notFound = $state(false);
	let stale = $state(false);

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
			} else if (isFromFuture(p)) {
				stale = true;
			} else {
				pipeline = p;
			}
		})();
	});

	const MAX_NAME_LENGTH = 64;
	const MIN_NAME_WIDTH_PX = 60;
	const NAME_EXTRA_PX = 4;

	let nameSaveTimer: ReturnType<typeof setTimeout> | undefined;
	function onNameInput() {
		if (pipeline && pipeline.name.length > MAX_NAME_LENGTH) {
			pipeline.name = pipeline.name.slice(0, MAX_NAME_LENGTH);
		}
		clearTimeout(nameSaveTimer);
		nameSaveTimer = setTimeout(() => {
			if (!pipeline) return;
			// Merge with the latest editor snapshot when one exists so we don't
			// clobber unsaved node/edge changes; otherwise persist the current
			// pipeline object with the new name.
			const snapshot = pipelineStore.editorActions?.getSnapshot();
			const next = snapshot ? { ...snapshot, name: pipeline.name } : { ...pipeline, updatedAt: Date.now() };
			pipelineStore.save(next);
		}, 500);
	}

	let nameInput = $state<HTMLInputElement | null>(null);
	let nameMirror = $state<HTMLSpanElement | null>(null);
	let nameWidth = $state(MIN_NAME_WIDTH_PX);

	$effect(() => {
		pipeline?.name;
		if (!nameMirror) return;
		let width = nameMirror.getBoundingClientRect().width;
		if (nameInput) {
			const cs = getComputedStyle(nameInput);
			width += parseFloat(cs.borderLeftWidth) + parseFloat(cs.borderRightWidth);
		}
		nameWidth = Math.max(MIN_NAME_WIDTH_PX, width + NAME_EXTRA_PX);
	});
</script>

<Toaster
	position="bottom-end"
	containerClassName="mr-72"
	toastOptions={{
		duration: 5000,
		className: 'bg-neutral-200! rounded-xl! text-neutral-900! px-3!'
	}} />

<Header>
	{#snippet left()}
		<div class="flex items-center gap-3">
			<a href="/" class="button-header px-4 text-sm">← Back</a>
			{#if pipeline}
				<div class="relative">
					<span bind:this={nameMirror} class="pointer-events-none invisible absolute top-0 left-0 px-2.5 whitespace-pre" aria-hidden="true">
						{pipeline.name}
					</span>
					<input
						bind:this={nameInput}
						bind:value={pipeline.name}
						oninput={onNameInput}
						maxlength={MAX_NAME_LENGTH}
						class="input-base !transition-none"
						style:width="{nameWidth}px" />
				</div>
			{/if}
		</div>
	{/snippet}

	{#snippet right()}
		<div class="flex items-center gap-3">
			{#if audioStore.isRunning}
				<RunningTimer />
				<LatencyBadge />
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
	{:else if stale}
		<div class="flex w-full flex-col items-start gap-4 p-8">
			<div class="warning-block max-w-xl">
				<span class="font-semibold">This pipeline was saved by a newer version of Splitwave.</span>
				<span>
					Its layout is not known to this build, so opening it would mean guessing at routing this version cannot represent. It is left untouched and
					can still be deleted.
				</span>
				<span>Update Splitwave to open it.</span>
			</div>
			<a href="/" class="button-main primary rounded-lg text-sm">Back to pipelines</a>
		</div>
	{:else if pipeline}
		<Flow.ui.Flow {pipeline} />
	{:else}
		<div class="p-8 text-sm text-gray-500">Loading…</div>
	{/if}
</div>
