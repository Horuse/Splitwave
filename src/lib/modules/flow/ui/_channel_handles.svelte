<script lang="ts">
	import { Handle, Position } from '@xyflow/svelte';
	import { channelColor, channelLabel, handleStyle } from '$lib/modules/flow/utils';
	import { Link } from '$lib/components/icons';

	interface Props {
		count: number;
		side: 'source' | 'target';
		stereoGroups?: number[];
		onToggleGroup?: (lower: number) => void;
	}
	let { count, side, stereoGroups = [], onToggleGroup }: Props = $props();

	let indices = $derived(Array.from({ length: count }, (_, i) => i));

	function isGrouped(lower: number): boolean {
		return stereoGroups.includes(lower);
	}
</script>

<div class="flex flex-col gap-0.5">
	{#each indices as i (i)}
		{#if side === 'source'}
			<div class="relative -mr-4 flex min-h-4 items-center justify-end gap-1 pr-4">
				<span class="font-mono text-[9px]" style="color:{channelColor(i)}">{channelLabel(i, count)}</span>
				<Handle type="source" id={`ch${i + 1}`} position={Position.Right} class="handle" style={handleStyle(channelColor(i))} />
			</div>
		{:else}
			<div class="relative -ml-4 flex min-h-4 items-center gap-1 pl-4">
				<Handle type="target" id={`ch${i + 1}`} position={Position.Left} class="handle" style={handleStyle(channelColor(i))} />
				<span class="font-mono text-[9px]" style="color:{channelColor(i)}">{channelLabel(i, count)}</span>
			</div>
		{/if}

		{#if onToggleGroup && i < count - 1}
			{@const lower = i + 1}
			{@const grouped = isGrouped(lower)}
			<div
				class={[
					'relative flex min-h-4 items-center gap-1',
					side === 'source' ? '-mr-4 justify-end pr-4' : '-ml-4 pl-4'
				]}
			>
				{#if side === 'target' && grouped}
					<Handle type="target" id={`st${lower}`} position={Position.Left} class="handle" style={handleStyle('#a3a3a3')} />
				{/if}
				{#if side === 'target'}
					<span
						class={[
							'pointer-events-none ml-4 -my-2.5 z-5 w-1.5 self-stretch rounded-r-sm border-y border-r transition-opacity',
							grouped ? 'border-neutral-900 opacity-100' : 'opacity-0'
						]}
					></span>
				{/if}
				<button
					type="button"
					class={[
						'nodrag nopan flex size-4 items-center justify-center rounded-full border transition-colors',
						grouped
							? 'border-neutral-900 bg-neutral-900 text-white'
							: 'border-neutral-300 bg-neutral-100 text-neutral-500 hover:bg-neutral-200 hover:text-neutral-900'
					]}
					title={grouped ? 'Unlink stereo pair' : `Link ch${lower}+ch${lower + 1} as stereo`}
					onclick={() => onToggleGroup(lower)}
				>
					<Link class="size-2.5" />
				</button>
				{#if side === 'source'}
					<span
						class={[
							'pointer-events-none ml-3 -my-2.25 z-5 w-1.5 self-stretch rounded-r-sm border-y border-r transition-opacity',
							grouped ? 'border-neutral-900 opacity-100' : 'opacity-0'
						]}
					></span>
				{/if}
				{#if side === 'source' && grouped}
					<Handle type="source" id={`st${lower}`} position={Position.Right} class="handle" style={handleStyle('#a3a3a3')} />
				{/if}
			</div>
		{/if}
	{/each}
</div>
