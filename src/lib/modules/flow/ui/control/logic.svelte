<script lang="ts">
	import { Segmented } from '@splitwave/ui';
	import { getContext, untrack } from 'svelte';
	import { useSvelteFlow, useUpdateNodeInternals, type Node, type NodeProps } from '@xyflow/svelte';
	import type { LogicNodeData } from '@splitwave/nodes';
	import Wrapper from '@splitwave/nodes/ui/node.svelte';
	import ArrowRight from '~icons/fluent/arrow-right-16-filled';
	import ControlPort from './_control_port.svelte';
	import GateGlyph from './_gate_glyph.svelte';
	import { A_HANDLE, B_HANDLE, CONTROL_OUT, PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { evaluate, LOGIC_OPS, usesB, type LogicOp } from '@splitwave/nodes';
	import { controlSignals, useControlInput } from '../../control.svelte';

	type LogicNodeType = Node<LogicNodeData, 'logic'>;
	let { id, data }: NodeProps<LogicNodeType> = $props();

	const flow = useSvelteFlow();
	const isPreview = getContext(PREVIEW_CTX) === true;
	const nodeId = untrack(() => id);
	const inA = isPreview ? null : useControlInput(nodeId, A_HANDLE);
	const inB = isPreview ? null : useControlInput(nodeId, B_HANDLE);

	let op = $derived<LogicOp>(data.op);
	let two = $derived(usesB(op));

	// A and B move when B comes or goes; xyflow remeasures sockets only on a
	// resize, which this may not cause, so cables would stay at the old spot.
	const updateNodeInternals = useUpdateNodeInternals();
	$effect(() => {
		two;
		updateNodeInternals(nodeId);
	});
	let a = $derived(inA?.current ?? false);
	let b = $derived(inB?.current ?? false);
	let out = $derived(evaluate(op, a, b));

	$effect(() => {
		controlSignals.emit(nodeId, CONTROL_OUT, out);
		return () => controlSignals.clear(nodeId, CONTROL_OUT);
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

<Wrapper label="Logic" icon={ArrowRight} accent="control">
	<div class="flex w-52 flex-col gap-3">
		<Segmented
			class="nodrag nopan"
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
			<GateGlyph {op} {a} {b} {out} />
			<div class="flex h-14 w-5 shrink-0 flex-col justify-center">
				<ControlPort id={CONTROL_OUT} side="source" label="" high={out} />
			</div>
		</div>
	</div>
</Wrapper>
