<script lang="ts">
	import { Position } from '@xyflow/svelte';
	import Handle from './_handle.svelte';
	import { controlEdgeStyle, controlTopStyle } from '$lib/modules/flow/utils';

	interface Props {
		id: string;
		side: 'source' | 'target';
		high: boolean;
		/** `top` puts a target on the node's top border instead of its side. */
		placement?: 'edge' | 'top';
		/** With `top`: px from the node's top border down to this socket's parent. */
		lift?: number;
	}
	let { id, side, high, placement = 'edge', lift = 0 }: Props = $props();

	let position = $derived(placement === 'top' ? Position.Top : side === 'source' ? Position.Right : Position.Left);
</script>

<Handle type={side} {id} {position} class="handle" style={placement === 'top' ? controlTopStyle(lift) : controlEdgeStyle(side)}>
	<span class={['socket', high && 'high']}></span>
</Handle>

<style>
	/* Rotated inside the handle: xyflow owns the handle's own transform. */
	.socket {
		pointer-events: none;
		position: absolute;
		inset: 2.5px;
		rotate: 45deg;
		border: 1.5px solid var(--color-neutral-750);
		background: var(--color-neutral-100);
	}
	.socket.high {
		border-color: var(--color-signal);
		background: var(--color-signal);
	}
</style>
