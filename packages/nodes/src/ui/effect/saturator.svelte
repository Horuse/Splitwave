<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { SaturatorNodeData } from '@splitwave/nodes';
	import ParamGraph from '../graph/param_graph.svelte';
	import Wrapper from '../node.svelte';
	import { Saturator } from '../icons';
	import PresetBar from '../preset_bar.svelte';
	import type { PresetData } from '../../presets';

	type SaturatorNodeType = Node<SaturatorNodeData, 'saturator'>;
	let { id, data }: NodeProps<SaturatorNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	function setThreshold(v: number) {
		const patch = { thresholdDb: v };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}
	function setDrive(v: number) {
		const patch = { driveDb: v };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}

	function applyPreset(p: PresetData<'saturator'>) {
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}

	// Transfer curve mirrors `SaturatorEffect` in effects.rs: y = c*tanh(x*d/c).
	let c = $derived(Math.pow(10, data.thresholdDb / 20));
	let d = $derived(Math.pow(10, data.driveDb / 20));

	function shape(v: number): number {
		return c * Math.tanh((v * d) / c);
	}

	const STEPS = 64;
	function transfer(sx: (v: number) => number, sy: (v: number) => number): string {
		return Array.from({ length: STEPS + 1 }, (_, i) => {
			const v = -1.2 + (2.4 * i) / STEPS;
			return `${sx(v).toFixed(1)},${sy(shape(v)).toFixed(1)}`;
		}).join(' ');
	}
</script>

<Wrapper label="Saturator" icon={Saturator} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="flex w-50 flex-col gap-1.5">
		<PresetBar kind="saturator" {data} onApply={applyPreset} />

		<div class="nowheel nodrag w-52">
			<ParamGraph x={[-1.2, 1.2]} y={[-1.2, 1.2]} xTicks={[-1, -0.5, 0, 0.5, 1]} yTicks={[-1, 0, 1]} xLabel="in" yLabel="out" height={96}>
				{#snippet children({ x, y })}
					<rect x="0" y="0" width="200" height={y(c)} class="fill-red-500/10" />
					<rect x="0" y={y(-c)} width="200" height="200" class="fill-red-500/10" />
					<line x1={x(-1.2)} y1={y(-1.2)} x2={x(1.2)} y2={y(1.2)} class="stroke-neutral-500" stroke-width="0.6" stroke-dasharray="2 2" />
					<polyline points={transfer(x, y)} fill="none" class="stroke-signal" stroke-width="1.5" />
				{/snippet}
			</ParamGraph>
		</div>

		<Slider
			class="nodrag nopan nowheel"
			label="Ceiling"
			info="The level the curve flattens toward. Peaks are rounded off smoothly near it instead of clipping hard."
			value={data.thresholdDb}
			min={-24}
			max={0}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[-12, -6, -3, 0]}
			onChange={setThreshold} />
		<Slider
			class="nodrag nopan nowheel"
			label="Drive"
			info="Pushes the signal into the curve. More drive means more rounding and more harmonics, so a warmer and denser sound."
			value={data.driveDb}
			min={0}
			max={24}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[0, 6, 12, 24]}
			onChange={setDrive} />
	</div>
</Wrapper>
