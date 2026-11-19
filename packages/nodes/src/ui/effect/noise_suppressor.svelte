<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NoiseSuppressorNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import Wand from '~icons/fluent/wand-16-filled';
	import PresetBar from '../preset_bar.svelte';
	import type { PresetData } from '../../presets';
	import { formatHz } from '@splitwave/nodes';

	type NoiseSuppressorNodeType = Node<NoiseSuppressorNodeData, 'noiseSuppressor'>;
	let { id, data }: NodeProps<NoiseSuppressorNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	function patch<K extends keyof NoiseSuppressorNodeData>(key: K, value: NoiseSuppressorNodeData[K]) {
		const p = { [key]: value } as Partial<NoiseSuppressorNodeData>;
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function applyPreset(p: PresetData<'noiseSuppressor'>) {
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
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

	let srcTooltip = $derived.by(() => {
		if (engine.sampleRate === 48_000) return undefined;
		return `Internal model runs at ${formatHz(48_000)} (resampled from ${formatHz(engine.sampleRate)} and back)`;
	});
</script>

<Wrapper
	label="Noise Suppressor"
	icon={Wand}
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	bypassed={data.bypassed}
	onBypass={toggleBypass}
	{srcTooltip}>
	<div class="flex w-52 flex-col gap-1.5">
		<PresetBar kind="noiseSuppressor" {data} onApply={applyPreset} />

		<p class="text-[10px] leading-tight text-neutral-400">DeepFilterNet speech denoise. {formatHz(48_000)} only.</p>
		<Slider
			class="nodrag nopan nowheel"
			label="Attenuation"
			info="Most the noise may be turned down, in dB. Lower values keep some room tone and sound more natural; 100 removes as much as possible."
			value={data.attenuationLimitDb}
			min={0}
			max={100}
			step={1}
			format={dbFmt}
			defaultValue={100}
			ticks={[25, 50, 75]}
			onChange={(v) => patch('attenuationLimitDb', v)} />
		<Slider
			class="nodrag nopan nowheel"
			label="Post-filter"
			info="Extra cleanup after the model for leftover noise between words. Small values are enough; high values can thin the voice."
			value={data.postFilterBeta ?? 0}
			min={0}
			max={0.05}
			step={0.005}
			format={pfFmt}
			defaultValue={0}
			ticks={[0.01, 0.02, 0.03, 0.04]}
			onChange={(v) => patch('postFilterBeta', v)} />
		<Slider
			class="nodrag nopan nowheel"
			label="Min thresh"
			info="Below this estimated signal-to-noise ratio the frame is treated as pure noise and fully suppressed."
			value={data.minThreshDb ?? -10}
			min={-15}
			max={35}
			step={1}
			format={threshFmt}
			defaultValue={-10}
			ticks={[0, 15, 25]}
			onChange={(v) => patch('minThreshDb', v)} />
		<Slider
			class="nodrag nopan nowheel"
			label="Max ERB thresh"
			info="Above this signal-to-noise ratio the coarse band stage is skipped, so clean speech passes unprocessed."
			value={data.maxErbThreshDb ?? 30}
			min={-15}
			max={35}
			step={1}
			format={threshFmt}
			defaultValue={30}
			ticks={[0, 15, 25]}
			onChange={(v) => patch('maxErbThreshDb', v)} />
		<Slider
			class="nodrag nopan nowheel"
			label="Max DF thresh"
			info="Above this signal-to-noise ratio the fine deep-filter stage is skipped. Keep it at or below Max ERB thresh."
			value={data.maxDfThreshDb ?? 20}
			min={-15}
			max={35}
			step={1}
			format={threshFmt}
			defaultValue={20}
			ticks={[0, 15, 25]}
			onChange={(v) => patch('maxDfThreshDb', v)} />
	</div>
</Wrapper>
