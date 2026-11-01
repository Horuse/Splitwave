<script lang="ts">
	import { Position } from '@xyflow/svelte';
	import Handle from '../_handle.svelte';
	import { controlEdgeStyle } from '$lib/modules/flow/utils';

	interface Props {
		id: string;
		side: 'source' | 'target';
		label: string;
		high?: boolean;
	}
	let { id, side, label, high = false }: Props = $props();

	let isSource = $derived(side === 'source');
</script>

<div class={['relative flex min-h-4 items-center gap-1.5 font-mono text-[9px] text-neutral-900', isSource && 'flex-row-reverse']}>
	<span class={['size-1.5 shrink-0 rounded-full transition-colors', high ? 'bg-yellow-500 shadow-[0_0_6px_rgba(234,179,8,0.8)]' : 'bg-neutral-400']}> </span>
	{label}
	<Handle type={side} {id} position={isSource ? Position.Right : Position.Left} class="handle" style={controlEdgeStyle(side)} />
</div>
