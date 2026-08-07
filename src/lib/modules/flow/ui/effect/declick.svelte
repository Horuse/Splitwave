<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { DeclickNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { Backspace } from '$lib/components/icons';
	import Slider from './_slider.svelte';

	type DeclickNodeType = Node<DeclickNodeData, 'declick'>;
	let { id, data }: NodeProps<DeclickNodeType> = $props();

	const flow = useSvelteFlow();

	function setSensitivity(v: number) {
		const patch = { sensitivity: v };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	function setMaxWidth(v: number) {
		const patch = { maxWidthMs: v };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
</script>

<Wrapper label="Declick" icon={Backspace} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="flex w-50 flex-col gap-1.5">
		<Slider
			label="Sensitivity"
			value={data.sensitivity}
			min={0}
			max={1}
			step={0.01}
			defaultValue={0.5}
			format={(v) => `${Math.round(v * 100)}%`}
			ticks={[0, 0.25, 0.5, 0.75, 1]}
			onChange={setSensitivity} />
		<Slider
			label="Max width"
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
