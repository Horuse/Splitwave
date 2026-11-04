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
		<button type="button" role="switch" aria-checked={on} aria-label="Toggle" class={['nodrag nopan relative flex h-[26px] w-[72px] cursor-pointer rounded-md border bg-neutral-100', on ? 'border-signal' : 'border-neutral-400']}
			onclick={flip}>
			<!-- The knob covers the state the switch is not in. -->
			<span class="flex flex-1 items-center justify-center font-mono text-[10px] font-semibold text-neutral-1000">OFF</span>
			<span class={['flex flex-1 items-center justify-center font-mono text-[10px] font-semibold', on ? 'text-signal' : 'text-neutral-1000']}>ON</span>
			<span class={['absolute inset-y-0.5 w-[calc(50%-3px)] rounded', on ? 'left-0.5 bg-signal' : 'left-[calc(50%+1px)] bg-neutral-400']}></span>
		</button>
		<ControlSocket id={CONTROL_OUT} side="source" high={on} />
	</div>
</Wrapper>
