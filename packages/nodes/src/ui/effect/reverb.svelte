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

	// The time axis follows the tail, so a small room is not a sliver in the corner.
	let T_MAX = $derived(Math.min(4, Math.max(0.5, rt60 * 1.3)));
	const BARS = 120;

	// Fixed pseudo-random sequences, so the tail looks like an impulse response
	// but stays still while nothing changes.
	function noise(seed: number): number[] {
		let v = seed;
		return Array.from({ length: BARS }, () => {
			v = (v * 16807) % 2147483647;
			return (v / 2147483647) * 2 - 1;
		});
	}
	const NOISE_L = noise(7);
	const NOISE_X = noise(1931);

	/** Damping takes the highs out of the tail; here, neighbouring bars average together. */
	function smooth(src: number[], amount: number): number[] {
		const k = Math.round(amount * 4);
		if (k === 0) return src;
		return src.map((_, i) => {
			let sum = 0;
			let n = 0;
			for (let j = -k; j <= k; j++) {
				const v = src[i + j];
				if (v !== undefined) {
					sum += v;
					n++;
				}
			}
			return (sum / n) * Math.sqrt(k + 1);
		});
	}

	let bars = $derived.by(() => {
		const left = smooth(NOISE_L, data.damping);
		const other = smooth(NOISE_X, data.damping);
		// Width 0: R equals L (mono tail). Width 1: R is independent.
		const right = left.map((v, i) => v * (1 - data.width) + other[i] * data.width);
		return left.map((l, i) => {
			const t = ((i + 0.5) / BARS) * T_MAX;
			// Height on a dB scale (0 at -60 dB): the tail falls 60 dB per RT60,
			// starting from the wet level.
			const db = (data.mix > 0 ? 20 * Math.log10(data.mix) : -60) - (60 * t) / rt60;
			const env = Math.max(0, 1 + db / 60);
			return { t, l: Math.abs(l) * env, r: Math.abs(right[i]) * env };
		});
	});
</script>

<Wrapper
	label="Reverb"
	icon={Reverb}
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
			<span>RT60 {rt60.toFixed(1)} s</span><span>mix {Math.round(data.mix * 100)}%</span>
		</div>
	{/snippet}

	<div class="flex flex-col gap-2">
		<PresetBar kind="reverb" {data} onApply={applyPreset} />

		<div class="nowheel nodrag w-52">
			<ParamGraph
				x={[0, T_MAX]}
				y={[-1, 1]}
				xTicks={T_MAX > 2 ? [1, 2, 3] : T_MAX > 1 ? [0.5, 1, 1.5] : [0.25, 0.5]}
				yTicks={[0]}
				yFormat={() => ''}
				xLabel="s"
				height={80}>
				{#snippet children({ x, y })}
					<line x1={x(0)} y1={y(1)} x2={x(0)} y2={y(-1)} class="stroke-neutral-600" stroke-width="2" />
					{#each bars as b (b.t)}
						<line x1={x(b.t)} y1={y(0)} x2={x(b.t)} y2={y(b.l)} class="stroke-signal" stroke-width="1.2" />
						<line x1={x(b.t)} y1={y(0)} x2={x(b.t)} y2={y(-b.r)} class="stroke-signal/60" stroke-width="1.2" />
					{/each}
					{#if rt60 <= T_MAX}
						<line x1={x(rt60)} y1="0" x2={x(rt60)} y2="200" class="stroke-neutral-500" stroke-width="0.6" stroke-dasharray="2 2" />
					{/if}
					<text x={x(0) + 4} y={y(0.85)} class="fill-neutral-500" font-size="6">L</text>
					<text x={x(0) + 4} y={y(-0.75)} class="fill-neutral-500" font-size="6">R</text>
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
				info="Size of the virtual room. Bigger rooms give a longer tail (RT60 on the graph)."
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
				info="How much the walls absorb high frequencies. More damping gives a darker, softer and shorter tail."
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
				info="Stereo spread of the reverb. 0% is a mono tail, 100% makes left and right tails fully different."
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
				info="Blend between the original and the reverb. 0% is dry only, 100% reverb only; the original gets quieter as the mix rises."
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
