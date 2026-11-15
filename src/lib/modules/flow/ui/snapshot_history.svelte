<script lang="ts">
	import { Button, Menu, MenuItem, MenuSection, Popover } from '@splitwave/ui';
	import History from '~icons/fluent/history-16-filled';
	import { onMount, onDestroy } from 'svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { methods as pipelineMethods, type Snapshot } from '$lib/modules/pipeline/methods';
	import ChevronDown from '~icons/fluent/chevron-down-16-filled';
	import { relativeTime } from '$lib/utils/time';

	let { pipelineId }: { pipelineId: string } = $props();

	let open = $state(false);
	let snapshots = $state<Snapshot[]>([]);

	async function refresh() {
		snapshots = await pipelineMethods.listSnapshots(pipelineId);
	}

	let refreshTimer: ReturnType<typeof setInterval> | undefined;
	onMount(() => {
		refresh();
		refreshTimer = setInterval(refresh, 5_000);
	});
	onDestroy(() => {
		if (refreshTimer !== undefined) clearInterval(refreshTimer);
	});

	$effect(() => {
		if (open) refresh();
	});

	function revert(snap: Snapshot) {
		pipelineStore.editorActions?.revertToSnapshot(snap.pipeline);
		open = false;
	}
</script>

<Popover bind:open placement="bottom-end" offsetPx={6}>
	{#snippet trigger()}
		<Button>
			<History class="size-4" />
			History
			<span class="text-neutral-900 tabular-nums">{snapshots.length}</span>
			<ChevronDown class="size-3 opacity-60" />
		</Button>
	{/snippet}

	<Menu class="w-72">
		<MenuSection label="Auto-saved snapshots" />
		<ul class="nodrag nopan nowheel flex max-h-72 flex-col overflow-y-auto">
			{#if snapshots.length === 0}
				<li class="px-2.5 py-2 text-xs text-neutral-900">No snapshots yet. Edit the graph to create one.</li>
			{:else}
				{#each [...snapshots].reverse() as snap, i (i)}
					<li>
						<MenuItem onclick={() => revert(snap)}>
							<span class="flex items-baseline justify-between gap-2">
								<span class="tabular-nums">{relativeTime(snap.takenAt)}</span>
								<span class="text-xs text-neutral-900">
									{snap.pipeline.nodes.length} nodes, {snap.pipeline.edges.length} edges
								</span>
							</span>
						</MenuItem>
					</li>
				{/each}
			{/if}
		</ul>
	</Menu>
</Popover>
