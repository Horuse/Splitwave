<script lang="ts">
	import { BaseEdge, getBezierPath, type EdgeProps } from '@xyflow/svelte';
	import { channelColor } from '$lib/modules/flow/utils';

	let {
		id,
		sourceX,
		sourceY,
		targetX,
		targetY,
		sourcePosition,
		targetPosition,
		sourceHandleId,
		markerEnd
	}: EdgeProps = $props();

	let channel = $derived.by(() => {
		if (!sourceHandleId?.startsWith('ch')) return null;
		const n = parseInt(sourceHandleId.slice(2), 10);
		return Number.isFinite(n) ? n - 1 : null;
	});

	let path = $derived(
		getBezierPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition })[0]
	);

	let style = $derived(
		channel !== null ? `stroke:${channelColor(channel)};stroke-width:2px` : undefined
	);
</script>

<BaseEdge {id} {path} {style} {markerEnd} />
