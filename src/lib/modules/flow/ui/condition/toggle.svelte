<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { ToggleNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { Loop } from '$lib/components/icons';
	import ControlPort from './_control_port.svelte';
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
	<div class="flex items-center gap-3">
		<ControlPort id={CONTROL_IN} side="target" label="flip" high={input?.current ?? false} />
		<button
			class={[
				'nodrag nopan rounded-md border px-3 py-1 font-mono text-xs transition-colors',
				on
					? 'border-yellow-500/60 bg-yellow-500/10 text-yellow-600 dark:text-yellow-400'
					: 'border-neutral-400 bg-neutral-100 text-neutral-1100 hover:bg-neutral-200'
			]}
			title="Flip"
			onclick={flip}>
			{on ? 'ON' : 'OFF'}
		</button>
		<ControlPort id={CONTROL_OUT} side="source" label="out" high={on} />
	</div>
</Wrapper>
