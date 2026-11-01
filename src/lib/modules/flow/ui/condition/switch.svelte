<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { SwitchNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import SegmentedButtons from '$lib/components/segmented_buttons.svelte';
	import InputColumn from './_input_column.svelte';
	import { ChevronDoubleRight } from '$lib/components/icons';
	import NumberStepper from '$lib/components/number_stepper.svelte';
	import ControlPort from './_control_port.svelte';
	import OutputRow from './_output_row.svelte';
	import { useSelectOutput } from './_select_output';
	import { PREVIEW_CTX, selectHandle } from '$lib/modules/flow/utils';
	import { MAX_BRANCHES, MIN_BRANCHES, pruneBranches, useBranchTriggers } from './_branches.svelte';

	type SwitchNodeType = Node<SwitchNodeData, 'switch'>;
	let { id, data }: NodeProps<SwitchNodeType> = $props();

	const flow = useSvelteFlow();
	const isPreview = getContext(PREVIEW_CTX) === true;
	const nodeId = untrack(() => id);
	const select = useSelectOutput(nodeId);
	const triggers = useBranchTriggers(nodeId, !isPreview, () => data.outputs, select);

	let outputs = $derived(Array.from({ length: data.outputs }, (_, i) => i + 1));

	function setOutputs(n: number) {
		const next = Math.min(Math.max(Math.round(n), MIN_BRANCHES), MAX_BRANCHES);
		const current = flow.getNode(nodeId)?.data as SwitchNodeData | undefined;
		if (!current || current.outputs === next) return;
		pruneBranches(flow, nodeId, next);
		flow.updateNodeData(nodeId, { outputs: next });
		if (current.selected > next) select(next);
	}
</script>

<Wrapper label="Switch" icon={ChevronDoubleRight} accent="condition">
	<div class="flex w-52 flex-col gap-3">
		<div class="nodrag nopan flex items-center justify-between gap-2">
			<span class="font-mono text-[9px] text-neutral-500">Outputs</span>
			<NumberStepper value={data.outputs} min={MIN_BRANCHES} max={MAX_BRANCHES} label="Outputs" onchange={setOutputs} />
		</div>
		<SegmentedButtons
			label="Open"
			options={outputs.map((k) => ({ label: String(k), value: k }))}
			columns={Math.min(outputs.length, 4)}
			value={data.selected}
			onSelect={select} />
		<InputColumn {nodeId} live={!isPreview} />
		<div class="flex flex-col gap-2">
			{#each outputs as k (k)}
				<div class="flex items-start justify-between gap-4">
					<ControlPort id={selectHandle(k)} side="target" label={`select ${k}`} high={triggers.high(k)} />
					<OutputRow {nodeId} branch={k} label={`Out ${k}`} open={data.selected === k} live={!isPreview} />
				</div>
			{/each}
		</div>
	</div>
</Wrapper>
