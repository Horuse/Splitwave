<script lang="ts">
	import { BaseEdge, getBezierPath, type EdgeProps } from '@xyflow/svelte';
	import { CONTROL_DASH, controlSignals, darken, handleColor, signalOf } from '@splitwave/nodes/ui';

	let { id, source, sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, sourceHandleId }: EdgeProps = $props();

	const PIN_W = 3;
	const PIN_H = 6;

	// Same look as the editor's cable: channel colour, pins on audio ends, dashed control.
	let control = $derived(signalOf(sourceHandleId) === 'control');
	let live = $derived(control && !!sourceHandleId && controlSignals.read(source, sourceHandleId));
	let color = $derived(live ? 'var(--color-signal)' : handleColor(sourceHandleId));
	let path = $derived(getBezierPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition })[0]);
</script>

<BaseEdge {id} {path} style="stroke:transparent" />
<g class="pointer-events-none">
	<path d={path} fill="none" stroke={color} stroke-width="2" stroke-dasharray={control ? CONTROL_DASH : undefined} />
	{#if !control}
		{#each [[sourceX, sourceY], [targetX, targetY]] as [x, y], end (end)}
			<rect x={x - PIN_W / 2} y={y - PIN_H / 2} width={PIN_W} height={PIN_H} rx="1" fill={color} stroke={darken(color)} stroke-width="0.5" />
		{/each}
	{/if}
</g>
