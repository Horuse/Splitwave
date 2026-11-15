<script lang="ts">
	import { Menu, MenuItem, MenuSection, Popover } from '@splitwave/ui';
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
		<button
			type="button"
			class="flex h-7 items-center gap-1.5 rounded-md border border-neutral-300 bg-neutral-100 px-3 text-xs text-neutral-1100 transition-colors hover:bg-neutral-200">
			<span>History</span>
			<span class="rounded bg-neutral-300 px-1 py-1 font-mono text-[10px] leading-none text-neutral-1000 tabular-nums">
				{snapshots.length}
			</span>
			<ChevronDown class="h-3 w-3 opacity-60" />
		</button>
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
