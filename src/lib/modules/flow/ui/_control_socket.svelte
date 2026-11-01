<script lang="ts">
	import { Position } from '@xyflow/svelte';
	import Handle from './_handle.svelte';
	import { controlEdgeStyle } from '$lib/modules/flow/utils';

	interface Props {
		id: string;
		side: 'source' | 'target';
		high: boolean;
	}
	let { id, side, high }: Props = $props();
</script>

<Handle type={side} {id} position={side === 'source' ? Position.Right : Position.Left} class="handle" style={controlEdgeStyle(side)}>
	<span
		class={[
			'pointer-events-none absolute inset-0.5 rounded-full transition-all duration-150',
			high ? 'bg-signal shadow-[0_0_6px_var(--color-signal)]' : 'bg-transparent'
		]}></span>
</Handle>
