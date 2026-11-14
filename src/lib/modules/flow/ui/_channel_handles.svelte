<script lang="ts">
	import { untrack } from 'svelte';
	import { Position, useNodeConnections, useUpdateNodeInternals } from '@xyflow/svelte';
	import Handle from './_handle.svelte';
	import { branchChannelOf, branchHandle, channelColor, channelHandle, channelLabel, deriveSlots, handleEdgeStyle } from '$lib/modules/flow/utils';
	import { channelCaps, channelSelection } from '$lib/modules/flow/stores.svelte';

	interface Props {
		nodeId: string;
		side: 'source' | 'target';
		max?: number;
		min?: number;
		/** Sources with nothing upstream (a net receiver) grow on their own cables. */
		selfGrowing?: boolean;
		/** Branch these sockets belong to, 1-based: an output of an If or Switch,
		 * or an input of a Select. */
		branch?: number;
		/** Names each socket (L / R / chN) beside it. */
		labelled?: boolean;
	}
	let { nodeId, side, max = Infinity, min = 0, selfGrowing = false, branch, labelled = false }: Props = $props();

	const updateNodeInternals = useUpdateNodeInternals();
	// The hook takes a value; a node's id is fixed for the component's lifetime.
	const id = untrack(() => nodeId);
	const incoming = useNodeConnections({ id, handleType: 'target' });
	const outgoing = useNodeConnections({ id, handleType: 'source' });

	let isSource = $derived(side === 'source');
	let grows = $derived(selfGrowing ? isSource : !isSource);
	// A channel only has an output once something feeds it, unless it is a source.
	// Cables into a branch count as that channel; a branched input counts only
	// its own branch's cables.
	let occupied = $derived(
		selfGrowing
			? outgoing.current.map((c) => c.sourceHandle).filter((h): h is string => !!h)
			: incoming.current
					.map((c) => branchChannelOf(c.targetHandle, isSource ? undefined : branch) ?? null)
					.filter((ch): ch is number => ch !== null)
					.map(channelHandle)
	);
	let fed = $derived(occupied.length);
	let slots = $derived(deriveSlots(occupied, grows, max, min).filter((s) => grows || s.occupied));

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
	// A multi-channel drag fans out plain channel handles, so branch outputs opt out.
	function onArm(event: MouseEvent, ch: number) {
		if (!event.altKey || !isSource || branch !== undefined) return;
		event.stopPropagation();
		event.preventDefault();
		channelSelection.toggle(nodeId, ch);
	}
</script>

<div class="flex flex-col gap-0.5">
	{#each slots as slot (slot.id)}
		{@const color = channelColor(slot.ch - 1)}
		{@const armed = isSource && channelSelection.has(nodeId, slot.ch)}
		<div class="relative flex min-h-4 items-center" class:justify-end={isSource} onmousedowncapture={(e) => onArm(e, slot.ch)}>
			{#if labelled && slot.occupied}
				<span class="font-mono text-[9px] leading-none" style="color:{color}">{channelLabel(slot.ch - 1, fed)}</span>
			{/if}
			<Handle
				type={side}
				id={branch === undefined ? slot.id : branchHandle(branch, slot.ch)}
				position={isSource ? Position.Right : Position.Left}
				class={['handle', armed && 'handle-armed']}
				style={handleEdgeStyle(color, side)} />
		</div>
	{/each}
</div>
