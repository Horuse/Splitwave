<script lang="ts">
	import { untrack } from 'svelte';
	import { Handle, Position, useNodeConnections, useUpdateNodeInternals } from '@xyflow/svelte';
	import {
		channelColor,
		deriveSlots,
		handleEdgeStyle,
		handleFreeStyle
	} from '$lib/modules/flow/utils';
	import { channelCaps, channelSelection } from '$lib/modules/flow/stores.svelte';

	interface Props {
		nodeId: string;
		side: 'source' | 'target';
		max?: number;
	}
	let { nodeId, side, max = Infinity }: Props = $props();

	const updateNodeInternals = useUpdateNodeInternals();
	// The hook takes a value; a node's id is fixed for the component's lifetime.
	const incoming = useNodeConnections({ id: untrack(() => nodeId), handleType: 'target' });

	let isSource = $derived(side === 'source');
	// A channel only has an output once something feeds it, so only the target grows.
	let occupied = $derived(
		incoming.current.map((c) => c.targetHandle).filter((h): h is string => !!h)
	);
	let slots = $derived(
		deriveSlots(occupied, !isSource, max).filter((s) => !isSource || s.occupied)
	);

	$effect(() => {
		if (isSource) return;
		channelCaps.set(nodeId, max);
		return () => channelCaps.delete(nodeId);
	});

	// xyflow remeasures handles only on resize; a slot added at equal height would not connect.
	$effect(() => {
		slots.length;
		updateNodeInternals(nodeId);
	});

	// Capture beats xyflow's mousedown, which would otherwise start a drag.
	function onArm(event: MouseEvent, ch: number) {
		if (!event.altKey || !isSource) return;
		event.stopPropagation();
		event.preventDefault();
		channelSelection.toggle(nodeId, ch);
	}
</script>

<div class="flex flex-col gap-0.5">
	{#each slots as slot (slot.id)}
		{@const color = channelColor(slot.ch - 1)}
		{@const armed = isSource && channelSelection.has(nodeId, slot.ch)}
		<div
			class="relative flex min-h-4 items-center gap-1"
			class:justify-end={isSource}
			onmousedowncapture={(e) => onArm(e, slot.ch)}
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
				class={['handle', armed && 'handle-armed']}
				style={slot.occupied ? handleEdgeStyle(color, side) : handleFreeStyle(side)}
			/>
		</div>
	{/each}
</div>
