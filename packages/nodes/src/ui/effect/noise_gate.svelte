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
	// Same rule as noise_gate.rs: above the threshold the gate is open; below it, a gate still
	// fully open is in its hold time, one on its way down is releasing.
	let closedGain = $derived(Math.pow(10, data.rangeDb / 20));
	let gateState = $derived<'open' | 'hold' | 'release' | 'closed'>(
		gr.inRawDb >= data.thresholdDb ? 'open' : gr.gain > 0.98 ? 'hold' : gr.gain > closedGain + 0.02 ? 'release' : 'closed'
	);

	const STATE = {
		open: { label: 'Open', class: 'text-emerald-500' },
		hold: { label: 'Hold', class: 'text-amber-500' },
		release: { label: 'Release', class: 'text-sky-500' },
		closed: { label: 'Closed', class: 'text-neutral-600' }
	} as const;

	const X_MIN = -80;
	const X_MAX = 0;

	// noise_gate.rs opens on a hard threshold: at or above it the gain is unity,
	// below it the signal drops by Range. There is no soft knee in level; the
	// smoothing is in time (attack, hold, release).
	function outAt(xDb: number): number {
		return xDb >= data.thresholdDb ? xDb : xDb + data.rangeDb;
	}

	function curve(sx: (v: number) => number, sy: (v: number) => number): string {
		const t = data.thresholdDb;
		return [`${sx(X_MIN)},${sy(X_MIN + data.rangeDb)}`, `${sx(t)},${sy(t + data.rangeDb)}`, `${sx(t)},${sy(t)}`, `${sx(X_MAX)},${sy(X_MAX)}`].join(' ');
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
	onBypass={toggleBypass}
	collapsed={data.collapsed}>
	{#snippet summary()}
		<div class="flex w-52 items-center justify-between gap-2 font-mono text-[10px] text-neutral-900 tabular-nums">
			<span class={STATE[gateState].class}>{STATE[gateState].label}</span><span
				>thr {data.thresholdDb.toFixed(0)} dB{gr.db > 0.2 ? ` · -${gr.db.toFixed(1)}` : ''}</span>
		</div>
	{/snippet}

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
					{#if gr.inDb !== null}
						<line x1={x(gr.inDb)} y1="0" x2={x(gr.inDb)} y2="200" class="stroke-emerald-500" stroke-width="1" />
						<circle cx={x(gr.inDb)} cy={y(outAt(gr.inDb))} r="2.5" class="fill-emerald-500 stroke-neutral-100" stroke-width="1" />
					{/if}
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
				info="The gate opens when the signal is at or above this level and closes below it. Set it just above the noise between words."
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
				info="How much the gate turns the signal down while closed. -80 dB is near silence; -10 dB only softens the gaps."
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
				info="How fast the gate opens when the signal crosses the threshold. Very short attack can click; a few ms is smooth."
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
				info="How long the gate stays open after the signal drops below the threshold, so word endings and short pauses are not chopped."
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
				info="How fast the gate closes after the hold time ends. Longer release fades out tails naturally."
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
