<script lang="ts">
	import { untrack } from 'svelte';
	import { Handle, Position, useNodeConnections, useUpdateNodeInternals } from '@xyflow/svelte';
	import {
		channelColor,
		deriveSlots,
		handleEdgeStyle,
		handleFreeStyle
	} from '$lib/modules/flow/utils';

	interface Props {
		nodeId: string;
		side: 'source' | 'target';
	}
	let { nodeId, side }: Props = $props();

	const updateNodeInternals = useUpdateNodeInternals();
	// A node's id is fixed for the component's lifetime; the hook takes a value.
	const incoming = useNodeConnections({ id: untrack(() => nodeId), handleType: 'target' });

	let isSource = $derived(side === 'source');
	// Both sides key off the incoming cables: a channel only has an output once
	// something feeds it, so only the target side trails a free slot to grow on.
	let occupied = $derived(
		incoming.current.map((c) => c.targetHandle).filter((h): h is string => !!h)
	);
	let slots = $derived(deriveSlots(occupied, !isSource).filter((s) => !isSource || s.occupied));

	// xyflow caches handle bounds and only remeasures on a resize; a slot that
	// appears without changing node height would stay unconnectable otherwise.
	$effect(() => {
		slots.length;
		updateNodeInternals(nodeId);
	});
</script>

<div class="flex flex-col gap-0.5">
	{#each slots as slot (slot.id)}
		{@const color = channelColor(slot.ch - 1)}
		<div
			class={['relative flex items-center gap-1', isSource && 'justify-end']}
			style="min-height:{slot.width}rem"
		>
			{#if slot.occupied}
				<div
					class="wire pointer-events-none absolute top-1/2 -translate-y-1/2"
					style="{isSource ? 'right' : 'left'}:-1rem; width:1rem; color:{color}"
				></div>
			{/if}
			<Handle
				type={side}
				id={slot.id}
				position={isSource ? Position.Right : Position.Left}
				class="handle"
				style={slot.occupied ? handleEdgeStyle(color, side) : handleFreeStyle(side)}
			/>
		</div>
	{/each}
</div>
