<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { LimiterNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';

	type LimiterNodeType = Node<LimiterNodeData, 'limiter'>;
	let { id, data }: NodeProps<LimiterNodeType> = $props();

	const flow = useSvelteFlow();

	function setCeiling(v: number) {
		const patch = { ceilingDb: v };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
	function setRelease(v: number) {
		const patch = { releaseMs: v };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
	// `lookaheadMs` is build-time only — the host bakes the resulting latency
	// into the DAG at pipeline build, so a live `updateEffect` won't apply it.
	function setLookahead(v: number) {
		flow.updateNodeData(id, { lookaheadMs: v });
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
</script>

<Wrapper
	label="Limiter"
	accent="effect"
	hasInput
	hasOutput
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex w-50 flex-col gap-1.5">
		<Slider
			label="Ceiling"
			value={data.ceilingDb}
			min={-12}
			max={0}
			step={0.1}
			unit=" dB"
			defaultValue={-0.3}
			ticks={[-6, -3, -1, 0]}
			onChange={setCeiling}
		/>
		<Slider
			label="Lookahead"
			value={data.lookaheadMs}
			min={1}
			max={20}
			step={0.5}
			unit=" ms"
			defaultValue={5}
			ticks={[2, 5, 10]}
			onChange={setLookahead}
		/>
		<Slider
			label="Release"
			value={data.releaseMs}
			min={10}
			max={500}
			step={5}
			unit=" ms"
			defaultValue={50}
			ticks={[50, 100, 250]}
			onChange={setRelease}
		/>
		<p class="text-[9px] text-neutral-500">Lookahead change rebuilds the pipeline.</p>
	</div>
</Wrapper>
