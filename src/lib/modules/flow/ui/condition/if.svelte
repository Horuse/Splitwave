<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { IfNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { ArrowRight } from '$lib/components/icons';
	import SegmentedButtons from '$lib/components/segmented_buttons.svelte';
	import ControlPort from './_control_port.svelte';
	import GateGlyph from './_gate_glyph.svelte';
	import InputColumn from './_input_column.svelte';
	import OutputRow from './_output_row.svelte';
	import { useSelectOutput } from './_select_output';
	import { A_HANDLE, B_HANDLE, PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { evaluate, LOGIC_OPS, usesB, type LogicOp } from '../../logic';
	import { useControlInput } from '../../control.svelte';

	type IfNodeType = Node<IfNodeData, 'if'>;
	let { id, data }: NodeProps<IfNodeType> = $props();

	const flow = useSvelteFlow();
	const isPreview = getContext(PREVIEW_CTX) === true;
	const nodeId = untrack(() => id);
	const inA = isPreview ? null : useControlInput(nodeId, A_HANDLE);
	const inB = isPreview ? null : useControlInput(nodeId, B_HANDLE);
	const select = useSelectOutput(nodeId);

	const OUTPUTS = [
		{ branch: 1, label: 'True' },
		{ branch: 2, label: 'False' }
	];

	let op = $derived<LogicOp>(data.op ?? 'a');
	let two = $derived(usesB(op));
	let a = $derived(inA?.current ?? false);
	let b = $derived(inB?.current ?? false);
	let wired = $derived(inA?.current != null || (two && inB?.current != null));
	let result = $derived(evaluate(op, a, b));

	$effect(() => {
		if (!wired) return;
		const branch = result ? 1 : 2;
		untrack(() => select(branch));
	});

	function setOp(next: LogicOp) {
		// A one-input operator hides B, so a cable left there would dangle.
		if (!usesB(next)) {
			const orphaned = flow
				.getEdges()
				.filter((e) => e.target === nodeId && e.targetHandle === B_HANDLE)
				.map((e) => ({ id: e.id }));
			if (orphaned.length > 0) flow.deleteElements({ edges: orphaned });
		}
		flow.updateNodeData(nodeId, { op: next });
	}
</script>

<Wrapper label="If" icon={ArrowRight} accent="condition">
	<div class="flex w-52 flex-col gap-3">
		<SegmentedButtons
			label="Operator"
			note={LOGIC_OPS.find((o) => o.op === op)?.hint}
			options={LOGIC_OPS.map((o) => ({ label: o.label, value: o.op }))}
			columns={4}
			value={op}
			onSelect={setOp} />

		<div class="flex items-center gap-1">
			<div class={['flex h-14 w-5 shrink-0 flex-col', two ? 'justify-between py-1.5' : 'justify-center']}>
				<ControlPort id={A_HANDLE} side="target" label="A" high={a} />
				{#if two}
					<ControlPort id={B_HANDLE} side="target" label="B" high={b} />
				{/if}
			</div>
			<GateGlyph {op} {a} {b} out={wired && result} route={data.selected} onRoute={wired ? null : select} />
		</div>

		<div class="flex items-start justify-between gap-4">
			<InputColumn {nodeId} live={!isPreview} />
			<div class="flex flex-col items-end gap-2">
				{#each OUTPUTS as o (o.branch)}
					<OutputRow {nodeId} branch={o.branch} label={o.label} open={data.selected === o.branch} live={!isPreview} />
				{/each}
			</div>
		</div>
	</div>
</Wrapper>
