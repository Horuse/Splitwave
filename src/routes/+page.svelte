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
    import { page } from '$app/state';
    import { audioStore } from '$lib/modules/audio/stores.svelte';
    import { methods as audioMethods } from '$lib/modules/audio/methods';
    import { RunningTimer } from '$lib/modules/audio/ui';
    import { platform } from '@tauri-apps/plugin-os';

    const isWindows = platform() === 'windows';

    let busy = $state<string | null>(null);

    async function toggle(id: string, event: Event) {
        event.stopPropagation();
        event.preventDefault();
        if (busy) return;
        busy = id;
        try {
            if (audioStore.isRunning) {
                await audioMethods.stopPipeline();
            } else {
                const p = await pipelineMethods.get(id);
                if (!p) return;
                await audioStore.activatePipeline(id, { nodes: p.nodes, edges: p.edges });
            }
        } catch (e) {
            audioStore.reportError(e);
        } finally {
            busy = null;
        }
    }
</script>

<Header>
    {#snippet left()}
        <div class="flex items-center gap-2">
            <a class:active={page.route.id === '/'} href="/" class="button-header px-4 text-sm">Pipelines</a>
            {#if !isWindows}
                <a class:active={page.route.id === '/virtual-devices'} href="/virtual-devices" class="button-header px-4 text-sm">Virtual devices</a>
            {/if}
        </div>
    {/snippet}
</Header>

<div class="flex flex-col gap-8 scrollbar p-8 h-[calc(100vh-40px)] overflow-y-auto">
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
                        <div class="flex items-center gap-2">
                            {#if audioStore.runningPipelineId === p.id}
                                <span class="size-2 rounded-full bg-green-500"></span>
                            {/if}
                            <span class="font-medium">{p.name}</span>
                            {#if audioStore.runningPipelineId === p.id}
                                <RunningTimer />
                            {/if}
                        </div>
                        <div class="text-xs text-neutral-900">
                            {p.nodes.length} nodes · updated {relativeTime(p.updatedAt)}
                        </div>
                    </a>
                    <div class="flex items-center gap-2 mx-4">
                        <button
                            class={[
                                'button-main primary px-4',
                                !audioStore.isRunning && 'green',
                                audioStore.isRunning && audioStore.runningPipelineId === p.id && 'red'
                            ]}
                            disabled={!!busy || (audioStore.isRunning && audioStore.runningPipelineId !== p.id)}
                            onclick={(e) => toggle(p.id, e)}
                        >
                            {#if busy === p.id}
                                …
                            {:else if audioStore.isRunning && audioStore.runningPipelineId === p.id}
                                Stop
                            {:else}
                                Activate
                            {/if}
                        </button>
                        <button
                                class="button-main red py-1.5"
                                onclick={(e) => remove(p.id, p.name, e)}
                                aria-label="Delete pipeline"
                        >
                            Delete
                        </button>
                    </div>
                </li>
            {/each}
        </ul>
    {/if}
</div>
