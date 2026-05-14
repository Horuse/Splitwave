<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { GainNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';

	type GainNodeType = Node<GainNodeData, 'gain'>;
	let { id, data }: NodeProps<GainNodeType> = $props();

	const flow = useSvelteFlow();

	function set(v: number) {
		const patch = { gainDb: v };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	function valueClass(db: number): string {
		if (db >= 12) return 'text-red-500';
		if (db >= 3) return 'text-amber-600';
		return 'text-emerald-700';
	}
</script>

<Wrapper
	label="Gain"
	accent="effect"
	hasInput
	hasOutput
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="w-50">
		<Slider
			label="Level"
			value={data.gainDb}
			min={-24}
			max={24}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[-12, -6, 0, 6, 12]}
			valueClass={valueClass(data.gainDb)}
			onChange={set}
		/>
	</div>
</Wrapper>
