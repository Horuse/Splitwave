<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { CompressorNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import { SIDECHAIN_HANDLE } from '../utils';
	import { Compressor } from '../icons';
	import GrBar from './_gr_bar.svelte';
	import ParamGraph from '../graph/param_graph.svelte';
	import { useGainReduction } from '../state/gain_reduction.svelte';
	import PresetBar from '../preset_bar.svelte';
	import type { PresetData } from '../../presets';

	type CompressorNodeType = Node<CompressorNodeData, 'compressor'>;
	let { id, data }: NodeProps<CompressorNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	function patch<K extends keyof CompressorNodeData>(key: K, value: CompressorNodeData[K]) {
		const p = { [key]: value } as Partial<CompressorNodeData>;
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function applyPreset(p: PresetData<'compressor'>) {
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function ratioFmt(v: number): string {
		return `${v.toFixed(1)}:1`;
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}

	const gr = useGainReduction(() => id);

	const X_MIN = -60;
	const X_MAX = 0;

	/** Reduction the static curve applies at input `xDb`, before makeup. */
	function reductionAt(xDb: number): number {
		const halfKnee = data.kneeDb / 2;
		const over = xDb - data.thresholdDb;
		if (data.kneeDb > 0 && over > -halfKnee && over < halfKnee) {
			const k = over + halfKnee;
			return ((1 - 1 / data.ratio) * k * k) / (2 * data.kneeDb);
		}
		return over > 0 ? over * (1 - 1 / data.ratio) : 0;
	}

	function outAt(xDb: number): number {
		return xDb - reductionAt(xDb) + data.makeupDb;
	}

	const STEPS = 90;
	function curve(sx: (v: number) => number, sy: (v: number) => number): string {
		return Array.from({ length: STEPS + 1 }, (_, i) => {
			const xDb = X_MIN + (i / STEPS) * (X_MAX - X_MIN);
			return `${sx(xDb).toFixed(1)},${sy(outAt(xDb)).toFixed(1)}`;
		}).join(' ');
	}
</script>

<Wrapper
	label="Compressor"
	icon={Compressor}
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	inputs={[{ id: SIDECHAIN_HANDLE, label: 'Sidechain', position: 'bottom' }]}
	bypassed={data.bypassed}
	onBypass={toggleBypass}>
	<div class="flex flex-col gap-2">
		<PresetBar kind="compressor" {data} onApply={applyPreset} />

		<div class="nowheel nodrag flex w-52 flex-col gap-1">
			<ParamGraph x={[X_MIN, X_MAX]} y={[X_MIN, 12]} xTicks={[-48, -36, -24, -12]} yTicks={[-48, -24, 0]} xLabel="in dB" yLabel="out">
				{#snippet children({ x, y })}
					{@const kneeLo = data.thresholdDb - data.kneeDb / 2}
					{@const kneeHi = data.thresholdDb + data.kneeDb / 2}
					{#if data.kneeDb > 0}
						<rect x={x(kneeLo)} y="0" width={x(kneeHi) - x(kneeLo)} height="200" class="fill-neutral-200" />
					{/if}
					<line x1={x(X_MIN)} y1={y(X_MIN)} x2={x(X_MAX)} y2={y(X_MAX)} class="stroke-neutral-500" stroke-width="0.6" stroke-dasharray="2 2" />
					<line
						x1={x(data.thresholdDb)}
						y1="0"
						x2={x(data.thresholdDb)}
						y2="200"
						class="stroke-amber-500"
						stroke-width="0.7"
						stroke-dasharray="2 2" />
					<polyline points={curve(x, y)} fill="none" class="stroke-signal" stroke-width="1.5" stroke-linejoin="round" />
					{#if gr.inDb !== null}
						<line x1={x(gr.inDb)} y1="0" x2={x(gr.inDb)} y2="200" class="stroke-emerald-500" stroke-width="1" />
						<circle cx={x(gr.inDb)} cy={y(outAt(gr.inDb))} r="2.5" class="fill-emerald-500 stroke-neutral-100" stroke-width="1" />
					{/if}
					{#if gr.inDb !== null && gr.db > 0.2}
						<line x1={x(gr.inDb)} y1={y(gr.inDb + data.makeupDb)} x2={x(gr.inDb)} y2={y(outAt(gr.inDb))} class="stroke-red-500" stroke-width="2" />
					{/if}
				{/snippet}
				{#snippet badge()}
					<span>
						thr {data.thresholdDb.toFixed(1)} · {data.ratio.toFixed(1)}:1
					</span>
				{/snippet}
			</ParamGraph>
			<GrBar nodeId={id} horizontal />
		</div>

		<div class="flex w-52 flex-col gap-1.5">
			<Slider
				class="nodrag nopan nowheel"
				label="Threshold"
				info="Level where compression starts. Anything louder than this gets turned down; quieter parts pass untouched. The green line on the graph shows where your signal is now."
				value={data.thresholdDb}
				min={-60}
				max={0}
				step={0.5}
				unit=" dB"
				defaultValue={-18}
				ticks={[-40, -20, -10]}
				onChange={(v) => patch('thresholdDb', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Ratio"
				info="How hard levels above the threshold are turned down. At 4:1, a signal 8 dB over the threshold comes out only 2 dB over it."
				value={data.ratio}
				min={1}
				max={20}
				step={0.1}
				format={ratioFmt}
				defaultValue={3}
				ticks={[2, 4, 8]}
				onChange={(v) => patch('ratio', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Attack"
				info="How fast the compressor reacts once the level crosses the threshold. Short attack catches transients; longer attack lets the punch of a hit through."
				value={data.attackMs}
				min={0.1}
				max={100}
				step={0.1}
				unit=" ms"
				defaultValue={10}
				ticks={[1, 10, 50]}
				onChange={(v) => patch('attackMs', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Release"
				info="How fast the compressor lets go after the level falls back. Too short can flutter, too long keeps the signal squashed."
				value={data.releaseMs}
				min={10}
				max={1000}
				step={5}
				unit=" ms"
				defaultValue={100}
				ticks={[50, 250, 500]}
				onChange={(v) => patch('releaseMs', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Knee"
				info="Width of the soft zone around the threshold where compression fades in gradually instead of starting at once. 0 dB is a hard knee."
				value={data.kneeDb}
				min={0}
				max={24}
				step={0.5}
				unit=" dB"
				defaultValue={6}
				ticks={[0, 6, 12]}
				onChange={(v) => patch('kneeDb', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Makeup"
				info="Gain added after compression, to win back the loudness the compressor took away."
				value={data.makeupDb}
				min={0}
				max={24}
				step={0.1}
				unit=" dB"
				defaultValue={0}
				ticks={[0, 6, 12]}
				onChange={(v) => patch('makeupDb', v)} />
		</div>
	</div>
</Wrapper>
