<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { ReverbNodeData } from '@splitwave/nodes';
	import ParamGraph from '../graph/param_graph.svelte';
	import Wrapper from '../node.svelte';
	import { Reverb } from '../icons';
	import PresetBar from '../preset_bar.svelte';
	import type { PresetData } from '../../presets';

	type ReverbNodeType = Node<ReverbNodeData, 'reverb'>;
	let { id, data }: NodeProps<ReverbNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	function patch<K extends keyof ReverbNodeData>(key: K, value: ReverbNodeData[K]) {
		const p = { [key]: value } as Partial<ReverbNodeData>;
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function pctFmt(v: number): string {
		return `${Math.round(v * 100)}%`;
	}

	function applyPreset(p: PresetData<'reverb'>) {
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}

	// Same RT60 approximation the graph always used: room size 0..1 maps to
	// about 0.1..3 s, and damping shortens the tail.
	let rt60 = $derived((0.1 + data.roomSize * 2.9) / (1 + data.damping * 1.5));
	let wetDb = $derived(data.mix > 0 ? 20 * Math.log10(data.mix) : -60);

	const T_MAX = 3;
	const FLOOR_DB = -60;

	function tail(sx: (v: number) => number, sy: (v: number) => number): string {
		// The tail falls 60 dB per RT60, from the wet level down to the floor.
		const end = Math.min(T_MAX, (rt60 * (wetDb - FLOOR_DB)) / 60);
		const pts = [`${sx(0)},${sy(FLOOR_DB)}`, `${sx(0)},${sy(wetDb)}`, `${sx(end)},${sy(FLOOR_DB)}`];
		return pts.join(' ');
	}
</script>

<Wrapper label="Reverb" icon={Reverb} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="flex flex-col gap-2">
		<PresetBar kind="reverb" {data} onApply={applyPreset} />

		<div class="nowheel nodrag w-52">
			<ParamGraph x={[0, T_MAX]} y={[FLOOR_DB, 0]} xTicks={[1, 2]} yTicks={[-48, -24, 0]} xLabel="s" yLabel="dB" height={72}>
				{#snippet children({ x, y })}
					<polygon points={tail(x, y)} class="fill-signal/20" />
					<line x1={x(0)} y1={y(wetDb)} x2={x(rt60)} y2={y(wetDb + FLOOR_DB)} class="stroke-signal" stroke-width="1.5" />
					{#if rt60 <= T_MAX}
						<line x1={x(rt60)} y1="0" x2={x(rt60)} y2="200" class="stroke-neutral-500" stroke-width="0.6" stroke-dasharray="2 2" />
					{/if}
				{/snippet}
				{#snippet badge()}
					<span>RT60 {rt60.toFixed(1)} s</span>
					<span>wet {Math.round(data.mix * 100)}%</span>
					<span>width {Math.round(data.width * 100)}%</span>
				{/snippet}
			</ParamGraph>
		</div>

		<div class="flex w-52 flex-col gap-1.5">
			<Slider
				class="nodrag nopan nowheel"
				label="Room"
				value={data.roomSize}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={0.5}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('roomSize', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Damping"
				value={data.damping}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={0.5}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('damping', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Width"
				value={data.width}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={1}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('width', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Mix"
				value={data.mix}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={0.33}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('mix', v)} />
		</div>
	</div>
</Wrapper>
