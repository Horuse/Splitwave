<script lang="ts">
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { GainNodeData } from '@splitwave/nodes';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Trending from '~icons/fluent/arrow-trending-16-filled';

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

	function tone(db: number) {
		if (db >= 12) return 'danger';
		if (db >= 3) return 'warning';
		return 'success';
	}
</script>

<Wrapper label="Gain" icon={Trending} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="nowheel nodrag w-48">
		<Slider
			class="nodrag nopan nowheel"
			label="Level"
			value={data.gainDb}
			min={-24}
			max={24}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[-12, -6, 0, 6, 12]}
			tone={tone(data.gainDb)}
			onChange={set} />
	</div>
</Wrapper>
