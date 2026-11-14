<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { SelectNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import ChannelHandles from '../_channel_handles.svelte';
	import SegmentedButtons from '$lib/components/segmented_buttons.svelte';
	import NumberStepper from '$lib/components/number_stepper.svelte';
	import { ArrowSwap } from '$lib/components/icons';
	import ControlPort from './_control_port.svelte';
	import { useSelectOutput } from './_select_output';
	import { MAX_BRANCHES, MIN_BRANCHES, pruneBranches, useBranchTriggers } from './_branches.svelte';
	import { PREVIEW_CTX, selectHandle } from '$lib/modules/flow/utils';

	type SelectNodeType = Node<SelectNodeData, 'select'>;
	let { id, data }: NodeProps<SelectNodeType> = $props();

	const flow = useSvelteFlow();
	const isPreview = getContext(PREVIEW_CTX) === true;
	const nodeId = untrack(() => id);
	const select = useSelectOutput(nodeId);
	const triggers = useBranchTriggers(nodeId, !isPreview, () => data.inputs, select);

	let inputs = $derived(Array.from({ length: data.inputs }, (_, i) => i + 1));

	function setInputs(n: number) {
		const next = Math.min(Math.max(Math.round(n), MIN_BRANCHES), MAX_BRANCHES);
		const current = flow.getNode(nodeId)?.data as SelectNodeData | undefined;
		if (!current || current.inputs === next) return;
		pruneBranches(flow, nodeId, next);
		flow.updateNodeData(nodeId, { inputs: next });
		if (current.selected > next) select(next);
	}
</script>

<Wrapper label="Select" icon={ArrowSwap} accent="condition">
	<div class="flex w-52 flex-col gap-3">
		<div class="nodrag nopan flex items-center justify-between gap-2">
			<span class="font-mono text-[9px] text-neutral-500">Inputs</span>
			<NumberStepper value={data.inputs} min={MIN_BRANCHES} max={MAX_BRANCHES} label="Inputs" onchange={setInputs} />
		</div>
		<SegmentedButtons
			label="Hear"
			options={inputs.map((k) => ({ label: String(k), value: k }))}
			columns={Math.min(inputs.length, 4)}
			value={data.selected}
			onSelect={select} />
		<div class="flex items-start justify-between gap-4">
			<div class="flex flex-col gap-2">
				{#each inputs as k (k)}
					<div class="flex flex-col gap-0.5">
						<ControlPort id={selectHandle(k)} side="target" label={`select ${k}`} high={triggers.high(k)} />
						<span class={['font-mono text-[9px]', data.selected === k ? 'text-yellow-600 dark:text-yellow-400' : 'text-neutral-500']}>In {k}</span>
						{#if !isPreview}
							<ChannelHandles {nodeId} side="target" branch={k} labelled />
						{/if}
					</div>
				{/each}
			</div>
			<div class="flex flex-col items-end gap-0.5">
				<span class="font-mono text-[9px] text-neutral-500">out</span>
				{#if !isPreview}
					<ChannelHandles {nodeId} side="source" labelled />
				{/if}
			</div>
		</div>
	</div>
</Wrapper>
