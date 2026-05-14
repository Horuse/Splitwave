<script lang="ts">
	import type { NodeKind } from '$lib/modules/pipeline/types';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { DND_MIME, categoryLabel, categoryOrder, kindsByCategory, registry } from '../utils/nodes';
	import { Add } from '$lib/components/icons';

	function onDragStart(event: DragEvent, kind: NodeKind) {
		if (!event.dataTransfer) return;
		event.dataTransfer.setData(DND_MIME, kind);
		event.dataTransfer.effectAllowed = 'move';
	}

	function onClickAdd(kind: NodeKind) {
		pipelineStore.editorActions?.addNode(kind);
	}
</script>

<aside
	class="flex w-72 flex-col  gap-5 overflow-y-auto border-l border-neutral-100 bg-background p-4"
>
	{#each categoryOrder as category (category)}
		<section class="flex flex-col gap-2">
			<h2 class="text-[10px] font-semibold tracking-wider text-neutral-1000 uppercase">
				{categoryLabel[category]}
			</h2>
			<ul class="flex flex-col gap-1.5">
				{#each kindsByCategory[category] as kind (kind)}
					{@const node = registry[kind]}
					<li
						draggable="true"
						ondragstart={(e) => onDragStart(e, kind)}
						class="group flex items-start justify-between gap-2 rounded-lg bg-neutral-100 px-3 py-2 hover:bg-neutral-200"
					>
						<div class="flex min-w-0 flex-col">
							<span class="text-sm font-medium text-theme">{node.label}</span>
							<span class="text-[11px] leading-tight text-neutral-900">{node.description}</span>
						</div>
						<button
							class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-neutral-1000 hover:bg-neutral-300"
							onclick={() => onClickAdd(kind)}
							aria-label={`Add ${node.label}`}
						>
							<Add class="h-4 w-4" />
						</button>
					</li>
				{/each}
			</ul>
		</section>
	{/each}

	<p class="mt-auto text-[10px] text-neutral-900">Click + to add, or drag onto canvas.</p>
</aside>
