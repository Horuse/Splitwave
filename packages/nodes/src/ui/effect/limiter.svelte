<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { LimiterNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import { Limiter } from '../icons';
	import PresetBar from '../preset_bar.svelte';
	import type { PresetData } from '../../presets';
	import GrBar from './_gr_bar.svelte';
	import ParamGraph from '../graph/param_graph.svelte';
	import { useGainReduction } from '../state/gain_reduction.svelte';

	type LimiterNodeType = Node<LimiterNodeData, 'limiter'>;
	let { id, data }: NodeProps<LimiterNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	function setCeiling(v: number) {
		const patch = { ceilingDb: v };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}
	function setRelease(v: number) {
		const patch = { releaseMs: v };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}
	function setLookahead(v: number) {
		flow.updateNodeData(id, { lookaheadMs: v });
	}

	function applyPreset(p: PresetData<'limiter'>) {
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}

	const gr = useGainReduction(() => id);

	const X_MIN = -24;
	const X_MAX = 6;

	function outAt(xDb: number): number {
		return Math.min(xDb, data.ceilingDb);
	}
</script>

<Wrapper label="Limiter" icon={Limiter} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="flex flex-col gap-2">
		<PresetBar kind="limiter" {data} onApply={applyPreset} />

		<div class="nowheel nodrag flex w-50 flex-col gap-1">
			<ParamGraph x={[X_MIN, X_MAX]} y={[X_MIN, X_MAX]} xTicks={[-18, -12, -6, 0]} yTicks={[-18, -12, -6, 0]} xLabel="in dB" yLabel="out" height={80}>
				{#snippet children({ x, y })}
					<rect x={x(data.ceilingDb)} y="0" width="200" height="200" class="fill-red-500/10" />
					<line x1={x(X_MIN)} y1={y(X_MIN)} x2={x(X_MAX)} y2={y(X_MAX)} class="stroke-neutral-500" stroke-width="0.6" stroke-dasharray="2 2" />
					<line
						x1={x(X_MIN)}
						y1={y(data.ceilingDb)}
						x2={x(X_MAX)}
						y2={y(data.ceilingDb)}
						class="stroke-red-500"
						stroke-width="0.7"
						stroke-dasharray="2 2" />
					<polyline
						points="{x(X_MIN)},{y(X_MIN)} {x(data.ceilingDb)},{y(data.ceilingDb)} {x(X_MAX)},{y(data.ceilingDb)}"
						fill="none"
						class="stroke-signal"
						stroke-width="1.5"
						stroke-linejoin="round" />
					{#if gr.inDb !== null}
						<line x1={x(gr.inDb)} y1="0" x2={x(gr.inDb)} y2="200" class="stroke-emerald-500" stroke-width="1" />
						<circle cx={x(gr.inDb)} cy={y(outAt(gr.inDb))} r="2.5" class="fill-emerald-500 stroke-neutral-100" stroke-width="1" />
					{/if}
				{/snippet}
				{#snippet badge()}
					<span class="text-red-500">ceil {data.ceilingDb.toFixed(1)} dB</span>
				{/snippet}
			</ParamGraph>
			<GrBar nodeId={id} horizontal />
		</div>

		<div class="flex w-50 flex-col gap-1.5">
			<Slider
				class="nodrag nopan nowheel"
				label="Ceiling"
				value={data.ceilingDb}
				min={-12}
				max={0}
				step={0.1}
				unit=" dB"
				defaultValue={-0.3}
				ticks={[-6, -3, -1, 0]}
				onChange={setCeiling} />
			<Slider
				class="nodrag nopan nowheel"
				label="Lookahead"
				value={data.lookaheadMs}
				min={1}
				max={20}
				step={0.5}
				unit=" ms"
				defaultValue={5}
				ticks={[2, 5, 10]}
				onChange={setLookahead} />
			<Slider
				class="nodrag nopan nowheel"
				label="Release"
				value={data.releaseMs}
				min={10}
				max={500}
				step={5}
				unit=" ms"
				defaultValue={50}
				ticks={[50, 100, 250]}
				onChange={setRelease} />
			<p class="text-[9px] text-neutral-500">Lookahead change rebuilds the pipeline.</p>
		</div>
	</div>
</Wrapper>
