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
    
    import Header from '$lib/components/layout/header.svelte';
</script>

<Header></Header>

<div class="flex flex-col gap-8 p-8">
    <div class="flex items-center justify-between">
        <h1 class="text-2xl font-semibold">Pipelines</h1>

        <button
                class="button-main primary p-6 py-2"
                onclick={createPipeline}
        >
            New pipeline
        </button>
    </div>

    {#if store.pipelines.length === 0}
        <p class="text-sm text-theme">No pipelines yet. Create one to get started.</p>
    {:else}
        <ul class="flex flex-col gap-4">
            {#each store.pipelines as p (p.id)}
                <li class="flex items-center bg-neutral-200 hover:bg-neutral-300 transition-colors p-4 rounded-2xl">
                    <a href={`/pipelines/${p.id}`} class="flex-1">
                        <div class="font-medium">{p.name}</div>
                        <div class="text-xs text-neutral-900">
                            {p.nodes.length} nodes · updated {formatDate(p.updatedAt)}
                        </div>
                    </a>
                    <button
                            class="button-main red py-1.5"
                            onclick={(e) => remove(p.id, e)}
                            aria-label="Delete pipeline"
                    >
                        Delete
                    </button>
                </li>
            {/each}
        </ul>
    {/if}
</div>
