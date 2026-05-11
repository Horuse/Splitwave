<script lang="ts">
	import { page } from '$app/state';
	import { SvelteFlowProvider } from '@xyflow/svelte';
	import type { Pipeline } from '$lib/domain/pipeline';
	import { useAppStore } from '$lib/stores/app-store.svelte';
	import FlowEditor from '$lib/components/FlowEditor.svelte';
	import NodeSidebar from '$lib/components/NodeSidebar.svelte';
	import ActivationButton from '$lib/components/ActivationButton.svelte';

	const store = useAppStore();

	let pipeline = $state<Pipeline | null>(null);
	let notFound = $state(false);

	$effect(() => {
		const id = page.params.id;
		if (!id) {
			notFound = true;
			return;
		}
		(async () => {
			const p = await store.repo.get(id);
			if (!p) {
				notFound = true;
			} else {
				pipeline = p;
			}
		})();
	});
</script>

<div class="flex h-screen flex-col">
	<header class="flex items-center justify-between border-b bg-white px-4 py-2">
		<div class="flex items-center gap-3">
			<a href="/" class="text-sm text-gray-600 hover:underline">← Back</a>
			{#if pipeline}
				<h1 class="font-medium">{pipeline.name}</h1>
			{/if}
		</div>
		<div class="flex items-center gap-3">
			{#if store.lastError}
				<span class="text-xs text-red-600">{store.lastError}</span>
			{/if}
			{#if pipeline}
				<ActivationButton />
			{/if}
		</div>
	</header>

	{#if notFound}
		<div class="p-8 text-sm text-gray-500">Pipeline not found.</div>
	{:else if pipeline}
		<SvelteFlowProvider>
			<div class="flex flex-1 overflow-hidden">
				<FlowEditor {pipeline} />
				<NodeSidebar />
			</div>
		</SvelteFlowProvider>
	{:else}
		<div class="p-8 text-sm text-gray-500">Loading…</div>
	{/if}
</div>
