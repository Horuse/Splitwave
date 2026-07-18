<script lang="ts">
	import { Handle, Position } from '@xyflow/svelte';
	import { channelColor, handleEdgeStyle } from '$lib/modules/flow/utils';

	interface Props {
		side: 'source' | 'target';
		lower: number;
		grouped: boolean;
		onToggle: (lower: number) => void;
	}
	let { side, lower, grouped, onToggle }: Props = $props();

	let isSource = $derived(side === 'source');
	// The pair is ch{lower} / ch{lower+1} (0-based lower-1 / lower).
	let upperColor = $derived(channelColor(lower - 1));
	let lowerColor = $derived(channelColor(lower));
</script>

<div class={['relative flex min-h-4 items-center gap-1', isSource && 'justify-end']}>
	{#if !isSource && grouped}
		<Handle
			type="target"
			id={`st${lower}`}
			position={Position.Left}
			class="handle"
			style={handleEdgeStyle('#a3a3a3', 'target')}
		/>
	{/if}

	<!-- Brace: each half starts at its channel bar (top/bottom, inward) and
	     curves out to the shared stereo handle at the node edge, like the
	     bar->handle wire. Dashed and animated toward the tip; each half wears
	     its channel's colour. -->
	{#if grouped}
		<svg
			class="pointer-events-none absolute top-1/2 z-5 -translate-y-1/2"
			style={isSource ? 'right: calc(-1rem + 3px);' : 'left: calc(-1rem + 3px);'}
			width="13.5"
			height="36"
			viewBox="0 0 12 31"
			fill="none"
		>
			{#if isSource}
				<path class="brace-wire" d="M0 1C5 1 3.5 15.5 11.502 15.5" stroke={upperColor} stroke-width="0.9" />
				<path class="brace-wire" d="M0 30C5 30 3.5 15.5 11.502 15.5" stroke={lowerColor} stroke-width="0.9" />
			{:else}
				<path class="brace-wire" d="M12 1C7 1 8.5 15.5 0.498 15.5" stroke={upperColor} stroke-width="0.9" />
				<path class="brace-wire" d="M12 30C7 30 8.5 15.5 0.498 15.5" stroke={lowerColor} stroke-width="0.9" />
			{/if}
		</svg>
	{/if}

	<button
		type="button"
		class={[
			'nodrag nopan flex h-3.5 items-center rounded border px-1 font-mono text-[8px] leading-none transition-colors',
			grouped
				? 'border-neutral-900 bg-neutral-900 text-white'
				: 'border-neutral-300 bg-neutral-100 text-neutral-500 hover:bg-neutral-200 hover:text-neutral-900'
		]}
		title={grouped ? 'Unlink stereo pair' : `Link ch${lower}+ch${lower + 1} as stereo`}
		onclick={() => onToggle(lower)}
	>
		{grouped ? 'STEREO' : 'MONO'}
	</button>

	{#if isSource && grouped}
		<Handle
			type="source"
			id={`st${lower}`}
			position={Position.Right}
			class="handle"
			style={handleEdgeStyle('#a3a3a3', 'source')}
		/>
	{/if}
</div>
