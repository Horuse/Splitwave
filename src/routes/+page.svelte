<script lang="ts">
    import { goto } from '$app/navigation';
    import { createId } from '@paralleldrive/cuid2';
    import { methods as pipelineMethods } from '$lib/modules/pipeline/methods';
    import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
    import { modalManager } from '$lib/modules/overlay/modal';
    import { ConfirmModal } from '$lib/modules/overlay/ui';
    import { relativeTime } from '$lib/utils/time';

    async function createPipeline() {
        const id = createId();
        const p = pipelineMethods.emptyPipeline(id, `Pipeline ${pipelineStore.pipelines.length + 1}`);
        await pipelineStore.save(p);
        await goto(`/pipelines/${id}`);
    }

    async function remove(id: string, name: string, event: Event) {
        event.stopPropagation();
        event.preventDefault();
        const ok = await modalManager.open<boolean>(`Delete "${name}"?`, ConfirmModal, {
            message: 'This pipeline and its snapshot history will be permanently removed.',
            confirmLabel: 'Delete',
            danger: true
        });
        if (ok) await pipelineStore.remove(id);
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

    {#if pipelineStore.pipelines.length === 0}
        <p class="text-sm text-theme">No pipelines yet. Create one to get started.</p>
    {:else}
        <ul class="flex flex-col gap-4">
            {#each pipelineStore.pipelines as p (p.id)}
                <li class="flex items-center bg-neutral-200 hover:bg-neutral-300 transition-colors rounded-2xl">
                    <a href={`/pipelines/${p.id}`} class="flex-1 p-4">
                        <div class="font-medium">{p.name}</div>
                        <div class="text-xs text-neutral-900">
                            {p.nodes.length} nodes · updated {relativeTime(p.updatedAt)}
                        </div>
                    </a>
                    <button
                            class="button-main red py-1.5 mx-4"
                            onclick={(e) => remove(p.id, p.name, e)}
                            aria-label="Delete pipeline"
                    >
                        Delete
                    </button>
                </li>
            {/each}
        </ul>
    {/if}
</div>
