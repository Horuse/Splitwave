<script lang="ts">
	import { Handle, Position } from '@xyflow/svelte';
	import { channelColor, channelLabel, handleEdgeStyle } from '$lib/modules/flow/utils';
	import StereoBracket from './_stereo_bracket.svelte';

	interface Props {
		count: number;
		side: 'source' | 'target';
		stereoGroups?: number[];
		onToggleGroup?: (lower: number) => void;
	}
	let { count, side, stereoGroups = [], onToggleGroup }: Props = $props();

	let indices = $derived(Array.from({ length: count }, (_, i) => i));
	let isSource = $derived(side === 'source');

	function isGrouped(lower: number): boolean {
		return stereoGroups.includes(lower);
	}
</script>

<div class="flex flex-col gap-0.5">
	{#each indices as i (i)}
		<div class={['relative flex min-h-4 items-center gap-1', isSource && 'justify-end']}>
			{#if isSource}
				<span class="font-mono text-[9px]" style="color:{channelColor(i)}">{channelLabel(i, count)}</span>
				<div class="wire pointer-events-none absolute top-1/2 -translate-y-1/2" style="right:-1rem; width:1rem; color:{channelColor(i)}"></div>
				<Handle type="source" id={`ch${i + 1}`} position={Position.Right} class="handle" style={handleEdgeStyle(channelColor(i), 'source')} />
			{:else}
				<Handle type="target" id={`ch${i + 1}`} position={Position.Left} class="handle" style={handleEdgeStyle(channelColor(i), 'target')} />
				<div class="wire pointer-events-none absolute top-1/2 -translate-y-1/2" style="left:-1rem; width:1rem; color:{channelColor(i)}"></div>
				<span class="font-mono text-[9px]" style="color:{channelColor(i)}">{channelLabel(i, count)}</span>
			{/if}
		</div>

		{#if onToggleGroup && i < count - 1}
			<StereoBracket {side} lower={i + 1} grouped={isGrouped(i + 1)} onToggle={onToggleGroup} />
		{/if}
	{/each}
</div>
