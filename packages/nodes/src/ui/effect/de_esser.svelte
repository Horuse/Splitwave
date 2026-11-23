<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { DeEsserNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import SoundWave from '~icons/fluent/sound-wave-circle-16-filled';
	import PresetBar from '../preset_bar.svelte';
	import type { PresetData } from '../../presets';
	import { formatHz } from '@splitwave/nodes';

	type DeEsserNodeType = Node<DeEsserNodeData, 'deEsser'>;
	let { id, data }: NodeProps<DeEsserNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	function set(patch: Partial<DeEsserNodeData>) {
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}

	function applyPreset(p: PresetData<'deEsser'>) {
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		set({ bypassed: !data.bypassed });
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
	collapsed={data.collapsed}>
	{#snippet summary()}
		<div class="flex w-52 items-center justify-between gap-2 font-mono text-[10px] text-neutral-900 tabular-nums">
			<span>{formatHz(data.frequency)}</span><span>thr {data.thresholdDb.toFixed(0)} dB</span><span>{data.ratio.toFixed(1)}:1</span>
		</div>
	{/snippet}

	<div class="flex w-50 flex-col gap-1.5">
		<PresetBar kind="deEsser" {data} onApply={applyPreset} />

		<Slider
			class="nodrag nopan nowheel"
			label="Frequency"
			info="Where the sibilant band starts. Only content above this frequency is compressed. 5 to 8 kHz covers most voices."
			value={data.frequency}
			min={2000}
			max={16000}
			step={50}
			unit="Hz"
			defaultValue={6500}
			format={formatHz}
			ticks={[4000, 6500, 9000, 12000]}
			onChange={(v) => set({ frequency: v })} />
		<Slider
			class="nodrag nopan nowheel"
			label="Threshold"
			info="Level of the high band where the de-esser starts turning it down. Lower catches more 's' sounds."
			value={data.thresholdDb}
			min={-80}
			max={0}
			step={0.5}
			unit=" dB"
			defaultValue={-30}
			ticks={[-60, -40, -20, 0]}
			onChange={(v) => set({ thresholdDb: v })} />
		<Slider
			class="nodrag nopan nowheel"
			label="Ratio"
			info="How hard the high band is turned down once it passes the threshold. The rest of the voice is not touched."
			value={data.ratio}
			min={1}
			max={12}
			step={0.1}
			unit=":1"
			defaultValue={4}
			ticks={[2, 4, 8, 12]}
			onChange={(v) => set({ ratio: v })} />
		<span class="text-[9px] leading-tight text-neutral-500">
			Compresses only the band above Frequency when it passes Threshold, so sibilance softens while the rest of the voice stays untouched.
		</span>
	</div>
</Wrapper>
