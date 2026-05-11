<script lang="ts">
	import { goto } from '$app/navigation';
	import { createId } from '@paralleldrive/cuid2';
	import { emptyPipeline } from '$lib/domain/pipeline';
	import { useAppStore } from '$lib/stores/app-store.svelte';

	const store = useAppStore();

	async function createPipeline() {
		const id = createId();
		const p = emptyPipeline(id, `Pipeline ${store.pipelines.length + 1}`);
		await store.savePipeline(p);
		await goto(`/pipelines/${id}`);
	}

	async function remove(id: string, event: Event) {
		event.stopPropagation();
		await store.deletePipeline(id);
	}

	function formatDate(ts: number): string {
		return new Date(ts).toLocaleString();
	}
</script>

<section class="mx-auto max-w-3xl p-8">
	<header class="mb-6 flex items-center justify-between">
		<h1 class="text-2xl font-semibold">Pipelines</h1>
		<button
			class="rounded-md border px-3 py-1.5 text-sm hover:bg-gray-50"
			onclick={createPipeline}
		>
			+ New pipeline
		</button>
	</header>

	{#if store.pipelines.length === 0}
		<p class="text-sm text-gray-500">No pipelines yet. Create one to get started.</p>
	{:else}
		<ul class="divide-y rounded-md border">
			{#each store.pipelines as p (p.id)}
				<li class="flex items-center justify-between p-3 hover:bg-gray-50">
					<a href={`/pipelines/${p.id}`} class="flex-1">
						<div class="font-medium">{p.name}</div>
						<div class="text-xs text-gray-500">
							{p.nodes.length} nodes · updated {formatDate(p.updatedAt)}
						</div>
					</a>
					<button
						class="rounded-md px-2 py-1 text-sm text-red-600 hover:bg-red-50"
						onclick={(e) => remove(p.id, e)}
						aria-label="Delete pipeline"
					>
						Delete
					</button>
				</li>
			{/each}
		</ul>
	{/if}
</section>
