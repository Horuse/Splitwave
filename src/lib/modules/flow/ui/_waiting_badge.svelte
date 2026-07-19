<script lang="ts">
	import { useStore } from '@xyflow/svelte';
	import { parseHandle } from '$lib/modules/flow/utils';

	interface Props {
		nodeId: string;
		/** Channels the node currently offers; edges beyond this have no handle. */
		available: number;
		side: 'source' | 'target';
	}
	let { nodeId, available, side }: Props = $props();

	const store = useStore();

	// Counted from our own edges, not from xyflow's handle measurements: those are
	// null until a node is measured, which would flag every edge on first paint.
	let waiting = $derived(
		store.edges.filter((e) => {
			const handle = side === 'source' ? e.sourceHandle : e.targetHandle;
			const owner = side === 'source' ? e.source : e.target;
			if (owner !== nodeId || !handle) return false;
			const ch = parseHandle(handle);
			return ch !== null && ch > available;
		}).length
	);
</script>

{#if waiting > 0}
	<span
		class="flex h-4 shrink-0 items-center rounded border border-amber-500/50 bg-amber-500/15 px-1.5 font-mono text-[9px] text-amber-600 dark:text-amber-300"
		title="{waiting} connection{waiting > 1 ? 's' : ''} point past this device's channels. Pick a device with more channels to restore {waiting >
		1
			? 'them'
			: 'it'}."
	>
		{waiting} waiting
	</span>
{/if}
