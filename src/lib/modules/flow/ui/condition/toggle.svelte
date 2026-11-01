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
	<div class="relative flex items-center justify-center gap-3">
		<ControlSocket id={CONTROL_IN} side="target" high={input?.current ?? false} />
		<button type="button" role="switch" aria-checked={on} aria-label="Toggle" class={['switch nodrag nopan', on && 'on']} onclick={flip}>
			<span class="label off">OFF</span>
			<span class="label on">ON</span>
			<span class="knob"></span>
		</button>
		<ControlSocket id={CONTROL_OUT} side="source" high={on} />
	</div>
</Wrapper>

<style>
	/* A two-position switch: the knob covers the state it is not in, so the
	   word left showing is the one the switch holds. */
	.switch {
		position: relative;
		display: flex;
		width: 72px;
		height: 26px;
		border-radius: 6px;
		border: 1px solid var(--color-neutral-400);
		background: var(--color-neutral-100);
		cursor: pointer;
		transition: border-color 140ms;
	}
	.switch.on {
		border-color: var(--color-signal);
	}
	.label {
		flex: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		font:
			600 10px ui-monospace,
			monospace;
		color: var(--color-neutral-1000);
	}
	.switch.on .label.on {
		color: var(--color-signal);
	}
	.knob {
		position: absolute;
		top: 2px;
		bottom: 2px;
		left: calc(50% + 1px);
		width: calc(50% - 3px);
		border-radius: 4px;
		background: var(--color-neutral-400);
		transition:
			left 160ms ease-out,
			background-color 140ms;
	}
	.switch.on .knob {
		left: 2px;
		background: var(--color-signal);
	}
	@media (prefers-reduced-motion: reduce) {
		.knob {
			transition: none;
		}
	}
</style>
