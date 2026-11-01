<script lang="ts">
	import { Position } from '@xyflow/svelte';
	import Handle from '../_handle.svelte';
	import { controlEdgeStyle } from '$lib/modules/flow/utils';

	interface Props {
		id: string;
		side: 'source' | 'target';
		label: string;
		/** Lights the label while the signal is high. */
		high?: boolean;
	}
	let { id, side, label, high = false }: Props = $props();

	let isSource = $derived(side === 'source');
</script>

<div
	class={[
		'relative flex min-h-4 items-center font-mono text-[9px]',
		isSource && 'justify-end',
		high ? 'text-yellow-600 dark:text-yellow-400' : 'text-neutral-700'
	]}>
	{label}
	<Handle type={side} {id} position={isSource ? Position.Right : Position.Left} class="handle" style={controlEdgeStyle(side)} />
</div>
