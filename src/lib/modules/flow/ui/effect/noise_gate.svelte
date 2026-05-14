<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NoiseGateNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';

	type NoiseGateNodeType = Node<NoiseGateNodeData, 'noiseGate'>;
	let { id, data }: NodeProps<NoiseGateNodeType> = $props();

	const flow = useSvelteFlow();

	function patch<K extends keyof NoiseGateNodeData>(key: K, value: NoiseGateNodeData[K]) {
		const p = { [key]: value } as Partial<NoiseGateNodeData>;
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}
</script>

<Wrapper
	label="Noise Gate"
	accent="effect"
	hasOutput
	outputLabel="OUT"
	inputs={[
		{ id: 'main', label: 'IN', position: 'left' },
		{ id: 'sidechain', label: 'Sidechain', position: 'bottom' }
	]}
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex w-52 flex-col gap-1.5">
		<Slider
			label="Threshold"
			value={data.thresholdDb}
			min={-80}
			max={0}
			step={0.5}
			unit=" dB"
			defaultValue={-40}
			ticks={[-60, -40, -20]}
			onChange={(v) => patch('thresholdDb', v)}
		/>
		<Slider
			label="Range"
			value={data.rangeDb}
			min={-80}
			max={0}
			step={0.5}
			unit=" dB"
			defaultValue={-40}
			ticks={[-60, -40, -20]}
			onChange={(v) => patch('rangeDb', v)}
		/>
		<Slider
			label="Attack"
			value={data.attackMs}
			min={0.1}
			max={50}
			step={0.1}
			unit=" ms"
			defaultValue={1}
			ticks={[1, 5, 20]}
			onChange={(v) => patch('attackMs', v)}
		/>
		<Slider
			label="Hold"
			value={data.holdMs}
			min={0}
			max={500}
			step={5}
			unit=" ms"
			defaultValue={50}
			ticks={[20, 100, 250]}
			onChange={(v) => patch('holdMs', v)}
		/>
		<Slider
			label="Release"
			value={data.releaseMs}
			min={10}
			max={1000}
			step={5}
			unit=" ms"
			defaultValue={200}
			ticks={[100, 300, 500]}
			onChange={(v) => patch('releaseMs', v)}
		/>
	</div>
</Wrapper>
