<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { DeEsserNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { SoundWave } from '$lib/components/icons';
	import { PresetBar } from '$lib/modules/preset/ui';
	import type { PresetData } from '$lib/modules/preset';
	import Slider from './_slider.svelte';

	type DeEsserNodeType = Node<DeEsserNodeData, 'deEsser'>;
	let { id, data }: NodeProps<DeEsserNodeType> = $props();

	const flow = useSvelteFlow();

	function set(patch: Partial<DeEsserNodeData>) {
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	function applyPreset(p: PresetData<'deEsser'>) {
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		set({ bypassed: !data.bypassed });
	}

	function fmtHz(v: number): string {
		return v >= 1000 ? `${(v / 1000).toFixed(1)} k` : `${Math.round(v)} `;
	}
</script>

<Wrapper
	label="De-esser"
	icon={SoundWave}
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex w-50 flex-col gap-1.5">
		<PresetBar kind="deEsser" {data} onApply={applyPreset} />

		<Slider
			label="Frequency"
			value={data.frequency}
			min={2000}
			max={16000}
			step={50}
			unit="Hz"
			defaultValue={6500}
			format={fmtHz}
			ticks={[4000, 6500, 9000, 12000]}
			onChange={(v) => set({ frequency: v })}
		/>
		<Slider
			label="Threshold"
			value={data.thresholdDb}
			min={-80}
			max={0}
			step={0.5}
			unit=" dB"
			defaultValue={-30}
			ticks={[-60, -40, -20, 0]}
			onChange={(v) => set({ thresholdDb: v })}
		/>
		<Slider
			label="Ratio"
			value={data.ratio}
			min={1}
			max={12}
			step={0.1}
			unit=":1"
			defaultValue={4}
			ticks={[2, 4, 8, 12]}
			onChange={(v) => set({ ratio: v })}
		/>
		<span class="text-[9px] leading-tight text-neutral-500">
			Compresses only the band above Frequency when it passes Threshold, so
			sibilance softens while the rest of the voice stays untouched.
		</span>
	</div>
</Wrapper>
