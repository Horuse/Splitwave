<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { CycleNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { ArrowRedo } from '$lib/components/icons';
	import NumberStepper from '$lib/components/number_stepper.svelte';
	import ControlPort from './_control_port.svelte';
	import { NEXT_HANDLE, PREVIEW_CTX, RESET_HANDLE, parsePort, stepHandle } from '$lib/modules/flow/utils';
	import { controlSignals, useControlInput } from '../../control.svelte';
	import { MAX_BRANCHES, MIN_BRANCHES } from './_branches.svelte';

	type CycleNodeType = Node<CycleNodeData, 'cycle'>;
	let { id, data }: NodeProps<CycleNodeType> = $props();

	const flow = useSvelteFlow();
	const isPreview = getContext(PREVIEW_CTX) === true;
	const nodeId = untrack(() => id);
	const next = isPreview ? null : useControlInput(nodeId, NEXT_HANDLE);
	const reset = isPreview ? null : useControlInput(nodeId, RESET_HANDLE);

	let steps = $derived(Array.from({ length: data.steps }, (_, i) => i + 1));

	function current(): CycleNodeData | undefined {
		return flow.getNode(nodeId)?.data as CycleNodeData | undefined;
	}

	function goTo(step: number) {
		flow.updateNodeData(nodeId, { step });
	}

	function advance() {
		const d = current();
		if (d) goTo((d.step % d.steps) + 1);
	}

	// Only a rising edge moves: a held key must not run through every step.
	let nextWasHigh = false;
	$effect(() => {
		const high = next?.current ?? false;
		if (high && !nextWasHigh) untrack(advance);
		nextWasHigh = high;
	});

	let resetWasHigh = false;
	$effect(() => {
		const high = reset?.current ?? false;
		if (high && !resetWasHigh) untrack(() => goTo(1));
		resetWasHigh = high;
	});

	$effect(() => {
		const n = data.steps;
		const on = data.step;
		for (let k = 1; k <= MAX_BRANCHES; k++) {
			if (k <= n) controlSignals.emit(nodeId, stepHandle(k), k === on);
			else controlSignals.clear(nodeId, stepHandle(k));
		}
	});
	$effect(() => () => {
		for (let k = 1; k <= MAX_BRANCHES; k++) controlSignals.clear(nodeId, stepHandle(k));
	});

	function setSteps(n: number) {
		const count = Math.min(Math.max(Math.round(n), MIN_BRANCHES), MAX_BRANCHES);
		const d = current();
		if (!d || d.steps === count) return;
		const dropped = flow
			.getEdges()
			.filter((e) => {
				if (e.source !== nodeId) return false;
				const p = parsePort(e.sourceHandle);
				const k = p?.signal === 'control' ? Number(/^step(\d+)$/.exec(p.name)?.[1]) : NaN;
				return k > count;
			})
			.map((e) => ({ id: e.id }));
		if (dropped.length > 0) flow.deleteElements({ edges: dropped });
		flow.updateNodeData(nodeId, { steps: count, step: d.step > count ? 1 : d.step });
	}
</script>

<Wrapper label="Cycle" icon={ArrowRedo} accent="condition">
	<div class="flex w-52 flex-col gap-3">
		<div class="nodrag nopan flex items-center justify-between gap-2">
			<span class="font-mono text-[9px] text-neutral-500">Steps</span>
			<NumberStepper value={data.steps} min={MIN_BRANCHES} max={MAX_BRANCHES} label="Steps" onchange={setSteps} />
		</div>
		<div class="flex items-start justify-between gap-4">
			<div class="flex flex-col gap-2">
				<ControlPort id={NEXT_HANDLE} side="target" label="next" high={next?.current ?? false} />
				<ControlPort id={RESET_HANDLE} side="target" label="reset" high={reset?.current ?? false} />
			</div>
			<div class="flex flex-col gap-2">
				{#each steps as k (k)}
					<div class="flex items-center justify-end gap-2">
						<button
							type="button"
							aria-label={`Go to step ${k}`}
							class={['nodrag nopan size-2.5 cursor-pointer rounded-full border', data.step === k ? 'border-signal bg-signal' : 'border-neutral-500 hover:bg-neutral-300']}
							onclick={() => goTo(k)}></button>
						<ControlPort id={stepHandle(k)} side="source" label={String(k)} high={data.step === k} />
					</div>
				{/each}
			</div>
		</div>
	</div>
</Wrapper>
