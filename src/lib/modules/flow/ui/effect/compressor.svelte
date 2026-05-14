<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { CompressorNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';

	type CompressorNodeType = Node<CompressorNodeData, 'compressor'>;
	let { id, data }: NodeProps<CompressorNodeType> = $props();

	const flow = useSvelteFlow();

	function patch<K extends keyof CompressorNodeData>(key: K, value: CompressorNodeData[K]) {
		const p = { [key]: value } as Partial<CompressorNodeData>;
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function ratioFmt(v: number): string {
		return `${v.toFixed(1)}:1`;
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}
</script>

<Wrapper
	label="Compressor"
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
			min={-60}
			max={0}
			step={0.5}
			unit=" dB"
			defaultValue={-18}
			ticks={[-40, -20, -10]}
			onChange={(v) => patch('thresholdDb', v)}
		/>
		<Slider
			label="Ratio"
			value={data.ratio}
			min={1}
			max={20}
			step={0.1}
			format={ratioFmt}
			defaultValue={3}
			ticks={[2, 4, 8]}
			onChange={(v) => patch('ratio', v)}
		/>
		<Slider
			label="Attack"
			value={data.attackMs}
			min={0.1}
			max={100}
			step={0.1}
			unit=" ms"
			defaultValue={10}
			ticks={[1, 10, 50]}
			onChange={(v) => patch('attackMs', v)}
		/>
		<Slider
			label="Release"
			value={data.releaseMs}
			min={10}
			max={1000}
			step={5}
			unit=" ms"
			defaultValue={100}
			ticks={[50, 250, 500]}
			onChange={(v) => patch('releaseMs', v)}
		/>
		<Slider
			label="Knee"
			value={data.kneeDb}
			min={0}
			max={24}
			step={0.5}
			unit=" dB"
			defaultValue={6}
			ticks={[0, 6, 12]}
			onChange={(v) => patch('kneeDb', v)}
		/>
		<Slider
			label="Makeup"
			value={data.makeupDb}
			min={0}
			max={24}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[0, 6, 12]}
			onChange={(v) => patch('makeupDb', v)}
		/>
	</div>
</Wrapper>
