<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { DeclickNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import Backspace from '~icons/fluent/backspace-24-filled';

	type DeclickNodeType = Node<DeclickNodeData, 'declick'>;
	let { id, data }: NodeProps<DeclickNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	function setSensitivity(v: number) {
		const patch = { sensitivity: v };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}

	function setMaxWidth(v: number) {
		const patch = { maxWidthMs: v };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}
</script>

<Wrapper
	label="Declick"
	icon={Backspace}
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	bypassed={data.bypassed}
	onBypass={toggleBypass}
	collapsed={data.collapsed}>
	{#snippet summary()}
		<div class="flex w-52 items-center justify-between gap-2 font-mono text-[10px] text-neutral-900 tabular-nums">
			<span>sensitivity {Math.round(data.sensitivity * 100)}%</span><span>{data.maxWidthMs.toFixed(1)} ms</span>
		</div>
	{/snippet}

	<div class="flex w-50 flex-col gap-1.5">
		<Slider
			class="nodrag nopan nowheel"
			label="Sensitivity"
			info="How easily a short burst counts as a click. Higher catches quieter clicks but may touch sharp consonants or percussion."
			value={data.sensitivity}
			min={0}
			max={1}
			step={0.01}
			defaultValue={0.5}
			format={(v) => `${Math.round(v * 100)}%`}
			ticks={[0, 0.25, 0.5, 0.75, 1]}
			onChange={setSensitivity} />
		<Slider
			class="nodrag nopan nowheel"
			label="Max width"
			info="Longest click that gets repaired. Wider settings fix longer pops but add that much delay to the signal."
			value={data.maxWidthMs ?? 2}
			min={0.3}
			max={5}
			step={0.1}
			unit=" ms"
			defaultValue={2}
			ticks={[1, 2, 3, 5]}
			onChange={setMaxWidth} />
		<span class="text-[9px] leading-tight text-neutral-500">
			Sensitivity: higher catches smaller spikes. Max width caps how long a repaired click can be.
		</span>
	</div>
</Wrapper>
