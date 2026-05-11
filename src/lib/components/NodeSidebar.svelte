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

<aside class="w-56 shrink-0 border-l bg-gray-50 p-3">
	<h2 class="mb-3 text-xs font-semibold tracking-wide text-gray-500 uppercase">Nodes</h2>
	<ul class="space-y-2">
		{#each nodeKinds as kind (kind)}
			<li
				draggable="true"
				ondragstart={(e) => onDragStart(e, kind)}
				class="flex items-center justify-between rounded-md border bg-white p-2 text-sm shadow-sm hover:bg-gray-50"
			>
				<span>{nodeRegistry[kind].label}</span>
				<button
					class="rounded px-2 py-0.5 text-xs text-gray-600 hover:bg-gray-100"
					onclick={() => onClickAdd(kind)}
					aria-label={`Add ${nodeRegistry[kind].label}`}
				>
					+
				</button>
			</li>
		{/each}
	</ul>
	<p class="mt-3 text-xs text-gray-400">Click + to add, or drag onto canvas</p>
</aside>
