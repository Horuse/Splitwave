<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { fly } from 'svelte/transition';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { methods as pipelineMethods, type Snapshot } from '$lib/modules/pipeline/methods';
	import { ChevronDown } from '$lib/components/icons';

	let { pipelineId }: { pipelineId: string } = $props();

	let open = $state(false);
	let snapshots = $state<Snapshot[]>([]);
	let containerEl = $state<HTMLDivElement>();

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
		if (!open) return;
		const onDocPointer = (e: PointerEvent) => {
			if (containerEl && !containerEl.contains(e.target as Node)) open = false;
		};
		document.addEventListener('pointerdown', onDocPointer);
		return () => document.removeEventListener('pointerdown', onDocPointer);
	});

	function formatTime(ms: number): string {
		const now = Date.now();
		const diff = now - ms;
		if (diff < 60_000) return `${Math.floor(diff / 1000)}s ago`;
		if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
		if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
		const d = new Date(ms);
		return `${d.getMonth() + 1}/${d.getDate()} ${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`;
	}

	function revert(snap: Snapshot) {
		pipelineStore.editorActions?.revertToSnapshot(snap.pipeline);
		open = false;
	}

	function toggle() {
		open = !open;
		if (open) refresh();
	}
</script>

<div bind:this={containerEl} class="relative">
	<button
		type="button"
		class="flex h-7 items-center gap-1.5 rounded-md border border-neutral-400 bg-neutral-100 px-3 text-xs text-neutral-1100 transition-colors hover:bg-neutral-200"
		onclick={toggle}
	>
		<span>History</span>
		<span class="rounded bg-neutral-300 px-1 py-px font-mono text-[10px] tabular-nums leading-none text-neutral-1000">
			{snapshots.length}
		</span>
		<ChevronDown class="h-3 w-3 opacity-60" />
	</button>

	{#if open}
		<div
			transition:fly={{ duration: 200, y: 5 }}
			class="absolute top-full right-0 z-50 mt-1 w-72 overflow-hidden rounded-md border border-neutral-400 bg-neutral-50 shadow-lg"
		>
			<div class="border-b border-neutral-300 bg-neutral-100 px-3 py-1.5 text-[10px] tracking-wider text-neutral-900 uppercase">
				Auto-saved snapshots
			</div>
			<ul class="nodrag nopan nowheel max-h-72 overflow-y-auto">
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
								<span class="font-mono tabular-nums">{formatTime(snap.takenAt)}</span>
								<span class="text-[10px] text-neutral-900">
									{snap.pipeline.nodes.length}n · {snap.pipeline.edges.length}e
								</span>
							</button>
						</li>
					{/each}
				{/if}
			</ul>
		</div>
	{/if}
</div>
