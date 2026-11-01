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
	import { PREVIEW_CTX, parsePort, selectHandle } from '$lib/modules/flow/utils';
	import { useControlInput } from '../../control.svelte';

	type SwitchNodeType = Node<SwitchNodeData, 'switch'>;
	let { id, data }: NodeProps<SwitchNodeType> = $props();

	const MIN_OUTPUTS = 2;
	const MAX_OUTPUTS = 8;

	const flow = useSvelteFlow();
	const isPreview = getContext(PREVIEW_CTX) === true;
	const nodeId = untrack(() => id);
	const select = useSelectOutput(nodeId);
	// Hooks are fixed at mount, so every output a Switch can grow to gets one.
	const triggers = isPreview ? [] : Array.from({ length: MAX_OUTPUTS }, (_, i) => useControlInput(nodeId, selectHandle(i + 1)));

	let outputs = $derived(Array.from({ length: data.outputs }, (_, i) => i + 1));

	// Only a rising edge selects: a held key must not fight a later press.
	const wasHigh: boolean[] = Array(MAX_OUTPUTS).fill(false);
	$effect(() => {
		triggers.forEach((t, i) => {
			const high = t.current ?? false;
			if (high && !wasHigh[i] && i < data.outputs) untrack(() => select(i + 1));
			wasHigh[i] = high;
		});
	});

	function setOutputs(n: number) {
		const next = Math.min(Math.max(Math.round(n), MIN_OUTPUTS), MAX_OUTPUTS);
		const current = flow.getNode(nodeId)?.data as SwitchNodeData | undefined;
		if (!current || current.outputs === next) return;
		// Cables on a removed output would point at a socket that no longer exists.
		const dropped = flow
			.getEdges()
			.filter((e) => {
				const handle = e.source === nodeId ? e.sourceHandle : e.target === nodeId ? e.targetHandle : null;
				const p = parsePort(handle);
				if (p?.signal === 'audio') return p.port.kind === 'branch' && p.port.branch > next;
				if (p?.signal === 'control') return outputs.some((k) => k > next && selectHandle(k) === handle);
				return false;
			})
			.map((e) => ({ id: e.id }));
		if (dropped.length > 0) flow.deleteElements({ edges: dropped });
		flow.updateNodeData(nodeId, { outputs: next });
		if (current.selected > next) select(next);
	}
</script>

<Wrapper label="Switch" icon={ChevronDoubleRight} accent="condition">
	<div class="flex w-52 flex-col gap-3">
		<div class="nodrag nopan flex items-center justify-between gap-2">
			<span class="font-mono text-[9px] text-neutral-500">Outputs</span>
			<NumberStepper value={data.outputs} min={MIN_OUTPUTS} max={MAX_OUTPUTS} label="Outputs" onchange={setOutputs} />
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
					<ControlPort id={selectHandle(k)} side="target" label={`select ${k}`} high={triggers[k - 1]?.current ?? false} />
					<OutputRow {nodeId} branch={k} label={`Out ${k}`} open={data.selected === k} live={!isPreview} />
				</div>
			{/each}
		</div>
	</div>
</Wrapper>
