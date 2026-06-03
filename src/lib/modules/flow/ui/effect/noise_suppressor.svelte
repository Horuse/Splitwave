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
	</div>
</Wrapper>
