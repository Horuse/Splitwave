<script lang="ts">
	import { DND_MIME, nodeKinds, nodeRegistry } from '$lib/flow/node-registry';
	import type { NodeKind } from '$lib/domain/audio-node';
	import { useAppStore } from '$lib/stores/app-store.svelte';

	const store = useAppStore();

	function onDragStart(event: DragEvent, kind: NodeKind) {
		if (!event.dataTransfer) return;
		event.dataTransfer.setData(DND_MIME, kind);
		event.dataTransfer.effectAllowed = 'move';
	}

	function onClickAdd(kind: NodeKind) {
		store.editorActions?.addNode(kind);
	}
</script>

<aside class="w-64 shrink-0 flex flex-col gap-3 border-l border-neutral-200 bg-background p-4">
	<h2 class="text-xs font-semibold tracking-wide text-neutral-800 uppercase">Nodes</h2>
	<ul class="flex flex-col gap-2">
		{#each nodeKinds as kind (kind)}
			<li
				draggable="true"
				ondragstart={(e) => onDragStart(e, kind)}
				class="bg-neutral-100 p-3 rounded-xl"
			>
				<span>{nodeRegistry[kind].label}</span>
				<button
					class="rounded px-2 py-0.5 text-xs text-neutral-800 hover:bg-gray-100"
					onclick={() => onClickAdd(kind)}
					aria-label={`Add ${nodeRegistry[kind].label}`}
				>
					+
				</button>
			</li>
		{/each}
	</ul>
	<p class="mt-3 text-xs text-neutral-800">Click + to add, or drag onto canvas</p>
</aside>
