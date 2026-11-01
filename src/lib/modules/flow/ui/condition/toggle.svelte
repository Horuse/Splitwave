<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { ToggleNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { Loop } from '$lib/components/icons';
	import ControlSocket from '../_control_socket.svelte';
	import { CONTROL_IN, CONTROL_OUT, PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { controlSignals, useControlInput } from '../../control.svelte';

	type ToggleNodeType = Node<ToggleNodeData, 'toggle'>;
	let { id, data }: NodeProps<ToggleNodeType> = $props();

	const flow = useSvelteFlow();
	const isPreview = getContext(PREVIEW_CTX) === true;
	const input = isPreview
		? null
		: useControlInput(
				untrack(() => id),
				CONTROL_IN
			);

	let on = $derived(data.on);

	function flip() {
		const current = flow.getNode(id)?.data as ToggleNodeData | undefined;
		if (current) flow.updateNodeData(id, { on: !current.on });
	}

	// Only a rising edge flips: holding the key must not toggle twice.
	let wasHigh = false;
	$effect(() => {
		const high = input?.current ?? false;
		if (high && !wasHigh) untrack(flip);
		wasHigh = high;
	});

	$effect(() => {
		controlSignals.emit(id, CONTROL_OUT, on);
		return () => controlSignals.clear(id, CONTROL_OUT);
	});
</script>

<Wrapper label="Toggle" icon={Loop} accent="condition">
	<div class="relative flex items-center justify-center gap-3 py-1">
		<ControlSocket id={CONTROL_IN} side="target" high={input?.current ?? false} />
		<span class={['led', on && 'on']} aria-hidden="true"></span>
		<button type="button" role="switch" aria-checked={on} aria-label="Toggle" class={['rocker nodrag nopan', on && 'on']} onclick={flip}>
			<span class="plate">
				<span class="mark off">O</span>
				<span class="mark on">I</span>
			</span>
		</button>
		<ControlSocket id={CONTROL_OUT} side="source" high={on} />
	</div>
</Wrapper>

<style>
	/* A panel rocker: the pressed half tips into the bezel, and the lit half
	   is the state the switch holds. */
	.rocker {
		width: 64px;
		height: 30px;
		padding: 3px;
		border-radius: 7px;
		background: #0e0e10;
		box-shadow:
			inset 0 1px 3px rgb(0 0 0 / 0.8),
			0 1px 0 rgb(255 255 255 / 0.06);
		perspective: 90px;
		cursor: pointer;
	}
	.plate {
		display: flex;
		width: 100%;
		height: 100%;
		border-radius: 5px;
		background: linear-gradient(to right, #1d1d1f, #2c2c2e 50%, #1d1d1f);
		box-shadow:
			inset 0 1px 0 rgb(255 255 255 / 0.08),
			0 1px 2px rgb(0 0 0 / 0.6);
		transform: rotateY(14deg);
		transition: transform 140ms cubic-bezier(0.3, 1.4, 0.5, 1);
	}
	.rocker.on .plate {
		transform: rotateY(-14deg);
	}
	.mark {
		flex: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		font:
			600 11px -apple-system,
			BlinkMacSystemFont,
			system-ui,
			sans-serif;
		color: #6e6e73;
		transition:
			color 140ms,
			text-shadow 140ms;
	}
	.rocker:not(.on) .mark.off {
		color: #d1d1d6;
	}
	.rocker.on .mark.on {
		color: var(--color-signal);
		text-shadow: 0 0 6px var(--color-signal);
	}
	.led {
		width: 7px;
		height: 7px;
		border-radius: 9999px;
		background: radial-gradient(circle at 35% 35%, #4a4a4c, #1a1a1c);
		box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.6);
		transition:
			background 140ms,
			box-shadow 140ms;
	}
	.led.on {
		background: radial-gradient(circle at 35% 35%, white, var(--color-signal) 55%);
		box-shadow: 0 0 8px var(--color-signal);
	}
	@media (prefers-reduced-motion: reduce) {
		.plate {
			transition: none;
		}
	}
</style>
