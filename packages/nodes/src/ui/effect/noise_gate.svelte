<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NoiseGateNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import { SIDECHAIN_HANDLE } from '../utils';
	import { NoiseGate } from '../icons';
	import PresetBar from '../preset_bar.svelte';
	import type { PresetData } from '../../presets';
	import ParamGraph from '../graph/param_graph.svelte';
	import { useGainReduction } from '../state/gain_reduction.svelte';

	type NoiseGateNodeType = Node<NoiseGateNodeData, 'noiseGate'>;
	let { id, data }: NodeProps<NoiseGateNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	function patch<K extends keyof NoiseGateNodeData>(key: K, value: NoiseGateNodeData[K]) {
		const p = { [key]: value } as Partial<NoiseGateNodeData>;
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function applyPreset(p: PresetData<'noiseGate'>) {
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}

	const gr = useGainReduction(() => id);
	let gateState = $derived<'open' | 'hold' | 'closed'>(gr.gain > 0.85 ? 'open' : gr.gain > 0.15 ? 'hold' : 'closed');

	const STATE = {
		open: { label: 'Open', class: 'text-emerald-500' },
		hold: { label: 'Hold', class: 'text-amber-500' },
		closed: { label: 'Closed', class: 'text-neutral-600' }
	} as const;

	const X_MIN = -80;
	const X_MAX = 0;
	// Width of the drawn transition below the threshold, dB.
	const ZONE = 3;

	function outAt(xDb: number): number {
		if (xDb >= data.thresholdDb) return xDb;
		const t = Math.max(0, Math.min(1, (xDb - (data.thresholdDb - ZONE)) / ZONE));
		return xDb + data.rangeDb * (1 - t * t);
	}

	const STEPS = 100;
	function curve(sx: (v: number) => number, sy: (v: number) => number): string {
		return Array.from({ length: STEPS + 1 }, (_, i) => {
			const xDb = X_MIN + (i / STEPS) * (X_MAX - X_MIN);
			return `${sx(xDb).toFixed(1)},${sy(outAt(xDb)).toFixed(1)}`;
		}).join(' ');
	}
</script>

<Wrapper
	label="Noise Gate"
	icon={NoiseGate}
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	inputs={[{ id: SIDECHAIN_HANDLE, label: 'Sidechain', position: 'bottom' }]}
	bypassed={data.bypassed}
	onBypass={toggleBypass}>
	<div class="flex flex-col gap-2">
		<PresetBar kind="noiseGate" {data} onApply={applyPreset} />

		<div class="nowheel nodrag w-52">
			<ParamGraph x={[X_MIN, X_MAX]} y={[X_MIN - 40, 6]} xTicks={[-60, -40, -20]} yTicks={[-80, -40, 0]} xLabel="in dB" yLabel="out">
				{#snippet children({ x, y })}
					<rect x={x(X_MIN)} y="0" width={x(data.thresholdDb) - x(X_MIN)} height="200" class="fill-neutral-200" />
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
				{/snippet}
				{#snippet badge()}
					<span class="flex items-center gap-1">
						<span class={STATE[gateState].class}>{STATE[gateState].label}</span>
						{#if gr.db > 0.2}<span class="text-neutral-800">-{gr.db.toFixed(1)}</span>{/if}
					</span>
				{/snippet}
			</ParamGraph>
		</div>

		<div class="flex w-52 flex-col gap-1.5">
			<Slider
				class="nodrag nopan nowheel"
				label="Threshold"
				value={data.thresholdDb}
				min={-80}
				max={0}
				step={0.5}
				unit=" dB"
				defaultValue={-40}
				ticks={[-60, -40, -20]}
				onChange={(v) => patch('thresholdDb', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Range"
				value={data.rangeDb}
				min={-80}
				max={0}
				step={0.5}
				unit=" dB"
				defaultValue={-40}
				ticks={[-60, -40, -20]}
				onChange={(v) => patch('rangeDb', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Attack"
				value={data.attackMs}
				min={0.1}
				max={50}
				step={0.1}
				unit=" ms"
				defaultValue={1}
				ticks={[1, 5, 20]}
				onChange={(v) => patch('attackMs', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Hold"
				value={data.holdMs}
				min={0}
				max={500}
				step={5}
				unit=" ms"
				defaultValue={50}
				ticks={[20, 100, 250]}
				onChange={(v) => patch('holdMs', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Release"
				value={data.releaseMs}
				min={10}
				max={1000}
				step={5}
				unit=" ms"
				defaultValue={200}
				ticks={[100, 300, 500]}
				onChange={(v) => patch('releaseMs', v)} />
		</div>
	</div>
</Wrapper>
