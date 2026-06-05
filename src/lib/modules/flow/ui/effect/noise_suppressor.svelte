<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NoiseSuppressorNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';

	type NoiseSuppressorNodeType = Node<NoiseSuppressorNodeData, 'noiseSuppressor'>;
	let { id, data }: NodeProps<NoiseSuppressorNodeType> = $props();

	const flow = useSvelteFlow();

	function patch<K extends keyof NoiseSuppressorNodeData>(key: K, value: NoiseSuppressorNodeData[K]) {
		const p = { [key]: value } as Partial<NoiseSuppressorNodeData>;
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}

	function dbFmt(v: number): string {
		return v >= 100 ? 'full' : `${Math.round(v)} dB`;
	}

	function pfFmt(v: number): string {
		return v <= 0 ? 'off' : v.toFixed(3);
	}

	function threshFmt(v: number): string {
		return `${Math.round(v)} dB`;
	}
</script>

<Wrapper
	label="Noise Suppressor"
	accent="effect"
	hasInput
	hasOutput
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex w-52 flex-col gap-1.5">
		<p class="text-[10px] leading-tight text-neutral-400">
			DeepFilterNet speech denoise. 48 kHz only.
		</p>
		<Slider
			label="Attenuation"
			value={data.attenuationLimitDb}
			min={0}
			max={100}
			step={1}
			format={dbFmt}
			defaultValue={100}
			ticks={[25, 50, 75]}
			onChange={(v) => patch('attenuationLimitDb', v)}
		/>
		<Slider
			label="Post-filter"
			value={data.postFilterBeta ?? 0}
			min={0}
			max={0.05}
			step={0.005}
			format={pfFmt}
			defaultValue={0}
			ticks={[0.01, 0.02, 0.03, 0.04]}
			onChange={(v) => patch('postFilterBeta', v)}
		/>
		<Slider
			label="Min thresh"
			value={data.minThreshDb ?? -10}
			min={-15}
			max={35}
			step={1}
			format={threshFmt}
			defaultValue={-10}
			ticks={[0, 15, 25]}
			onChange={(v) => patch('minThreshDb', v)}
		/>
		<Slider
			label="Max ERB thresh"
			value={data.maxErbThreshDb ?? 30}
			min={-15}
			max={35}
			step={1}
			format={threshFmt}
			defaultValue={30}
			ticks={[0, 15, 25]}
			onChange={(v) => patch('maxErbThreshDb', v)}
		/>
		<Slider
			label="Max DF thresh"
			value={data.maxDfThreshDb ?? 20}
			min={-15}
			max={35}
			step={1}
			format={threshFmt}
			defaultValue={20}
			ticks={[0, 15, 25]}
			onChange={(v) => patch('maxDfThreshDb', v)}
		/>
	</div>
</Wrapper>
