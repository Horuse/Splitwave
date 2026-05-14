<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { DelayNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';

	type DelayNodeType = Node<DelayNodeData, 'delay'>;
	let { id, data }: NodeProps<DelayNodeType> = $props();

	const flow = useSvelteFlow();

	function patch<K extends keyof DelayNodeData>(key: K, value: DelayNodeData[K]) {
		const p = { [key]: value } as Partial<DelayNodeData>;
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
	label="Delay"
	accent="effect"
	hasInput
	hasOutput
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex w-52 flex-col gap-1.5">
		<Slider
			label="Time"
			value={data.timeMs}
			min={1}
			max={2000}
			step={1}
			unit=" ms"
			defaultValue={250}
			ticks={[100, 500, 1000]}
			onChange={(v) => patch('timeMs', v)}
		/>
		<Slider
			label="Feedback"
			value={data.feedback}
			min={0}
			max={0.95}
			step={0.01}
			format={pctFmt}
			defaultValue={0.4}
			ticks={[0.25, 0.5, 0.75]}
			onChange={(v) => patch('feedback', v)}
		/>
		<Slider
			label="Mix"
			value={data.mix}
			min={0}
			max={1}
			step={0.01}
			format={pctFmt}
			defaultValue={0.35}
			ticks={[0.25, 0.5, 0.75]}
			onChange={(v) => patch('mix', v)}
		/>
	</div>
</Wrapper>
