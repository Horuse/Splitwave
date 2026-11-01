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
	<span class={['socket', high && 'high']}></span>
</Handle>

<style>
	/* Rotated inside the handle: xyflow owns the handle's own transform. */
	.socket {
		pointer-events: none;
		position: absolute;
		inset: 2.5px;
		rotate: 45deg;
		border-radius: 2.5px;
		border: 1.5px solid var(--color-neutral-750);
		background: var(--color-neutral-100);
		transition:
			background-color 150ms,
			border-color 150ms,
			box-shadow 150ms;
	}
	.socket.high {
		border-color: var(--color-signal);
		background: var(--color-signal);
		box-shadow: 0 0 6px var(--color-signal);
	}
</style>
