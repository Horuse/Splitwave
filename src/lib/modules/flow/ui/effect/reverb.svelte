<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { ReverbNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';

	type ReverbNodeType = Node<ReverbNodeData, 'reverb'>;
	let { id, data }: NodeProps<ReverbNodeType> = $props();

	const flow = useSvelteFlow();

	function patch<K extends keyof ReverbNodeData>(key: K, value: ReverbNodeData[K]) {
		const p = { [key]: value } as Partial<ReverbNodeData>;
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function pctFmt(v: number): string {
		return `${Math.round(v * 100)}%`;
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}
</script>

<Wrapper
	label="Reverb"
	accent="effect"
	hasInput
	hasOutput
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex w-52 flex-col gap-1.5">
		<Slider
			label="Room"
			value={data.roomSize}
			min={0}
			max={1}
			step={0.01}
			format={pctFmt}
			defaultValue={0.5}
			ticks={[0.25, 0.5, 0.75]}
			onChange={(v) => patch('roomSize', v)}
		/>
		<Slider
			label="Damping"
			value={data.damping}
			min={0}
			max={1}
			step={0.01}
			format={pctFmt}
			defaultValue={0.5}
			ticks={[0.25, 0.5, 0.75]}
			onChange={(v) => patch('damping', v)}
		/>
		<Slider
			label="Width"
			value={data.width}
			min={0}
			max={1}
			step={0.01}
			format={pctFmt}
			defaultValue={1}
			ticks={[0.25, 0.5, 0.75]}
			onChange={(v) => patch('width', v)}
		/>
		<Slider
			label="Mix"
			value={data.mix}
			min={0}
			max={1}
			step={0.01}
			format={pctFmt}
			defaultValue={0.33}
			ticks={[0.25, 0.5, 0.75]}
			onChange={(v) => patch('mix', v)}
		/>
	</div>
</Wrapper>
