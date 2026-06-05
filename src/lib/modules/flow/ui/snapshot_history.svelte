<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { methods as pipelineMethods, type Snapshot } from '$lib/modules/pipeline/methods';
	import { ChevronDown } from '$lib/components/icons';
	import { Popover } from '$lib/modules/overlay/ui';
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
			class="flex h-7 items-center gap-1.5 rounded-md border border-neutral-300 bg-neutral-100 px-3 text-xs text-neutral-1100 transition-colors hover:bg-neutral-200"
		>
			<span>History</span>
			<span class="rounded bg-neutral-300 px-1 py-1 font-mono text-[10px] tabular-nums leading-none text-neutral-1000">
				{snapshots.length}
			</span>
			<ChevronDown class="h-3 w-3 opacity-60" />
		</button>
	{/snippet}

	<div class="w-72 overflow-hidden rounded-md border border-neutral-400 bg-neutral-50 shadow-lg">
		<div class="border-b border-neutral-300 bg-neutral-100 px-3 py-1.5 text-[10px] tracking-wider text-neutral-900 uppercase">
			Auto-saved snapshots
		</div>
		<ul class="nodrag nopan nowheel max-h-72 scrollbar overflow-y-auto">
			{#if snapshots.length === 0}
				<li class="px-3 py-3 text-xs text-neutral-900 italic">
					No snapshots yet. Edit the graph to create one.
				</li>
			{:else}
				{#each [...snapshots].reverse() as snap (snap.takenAt)}
					<li>
						<button
							type="button"
							class="flex w-full items-baseline justify-between px-3 py-1.5 text-left text-xs text-neutral-1000 transition-colors hover:bg-neutral-200"
							onclick={() => revert(snap)}
						>
							<span class="font-mono tabular-nums">{relativeTime(snap.takenAt)}</span>
							<span class="text-[10px] text-neutral-900">
								{snap.pipeline.nodes.length}n · {snap.pipeline.edges.length}e
							</span>
						</button>
					</li>
				{/each}
			{/if}
		</ul>
	</div>
</Popover>
