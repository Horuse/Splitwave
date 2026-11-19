<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { DeEsserNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import ParamGraph from '../graph/param_graph.svelte';
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

	// The engine splits at `frequency` with an LR4 crossover and compresses only
	// the high band, so the band graph is the LR4 high-pass response.
	const F_LO = Math.log10(1000);
	const F_HI = Math.log10(20000);
	const BAND_FLOOR = -36;

	function hpfDb(f: number): number {
		const r4 = Math.pow(f / data.frequency, 4);
		return 20 * Math.log10(r4 / (1 + r4));
	}

	function band(sx: (v: number) => number, sy: (v: number) => number): string {
		const pts: string[] = [];
		for (let i = 0; i <= 80; i++) {
			const lf = F_LO + ((F_HI - F_LO) * i) / 80;
			pts.push(`${sx(lf).toFixed(1)},${sy(Math.max(BAND_FLOOR, hpfDb(Math.pow(10, lf)))).toFixed(1)}`);
		}
		return pts.join(' ');
	}

	function bandOut(xDb: number): number {
		return xDb > data.thresholdDb ? data.thresholdDb + (xDb - data.thresholdDb) / data.ratio : xDb;
	}

	function khz(lf: number): string {
		const f = Math.pow(10, lf);
		return f >= 1000 ? `${Math.round(f / 1000)}k` : `${Math.round(f)}`;
	}
</script>

<Wrapper label="De-esser" icon={SoundWave} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="flex w-50 flex-col gap-1.5">
		<PresetBar kind="deEsser" {data} onApply={applyPreset} />

		<div class="nowheel nodrag grid grid-cols-2 gap-2">
			<ParamGraph
				x={[F_LO, F_HI]}
				y={[BAND_FLOOR, 3]}
				xTicks={[Math.log10(2000), Math.log10(5000), Math.log10(10000)]}
				xFormat={khz}
				yTicks={[-24, 0]}
				xLabel="band"
				width={110}
				height={90}>
				{#snippet children({ x, y })}
					<polygon points="{band(x, y)} {x(F_HI)},{y(BAND_FLOOR)} {x(F_LO)},{y(BAND_FLOOR)}" class="fill-signal/15" />
					<polyline points={band(x, y)} fill="none" class="stroke-signal" stroke-width="1.5" />
					<line
						x1={x(Math.log10(data.frequency))}
						y1="0"
						x2={x(Math.log10(data.frequency))}
						y2="200"
						class="stroke-amber-500"
						stroke-width="0.7"
						stroke-dasharray="2 2" />
				{/snippet}
			</ParamGraph>
			<ParamGraph x={[-60, 0]} y={[-60, 0]} xTicks={[-40, -20]} yTicks={[-40, -20]} xLabel="band dB" width={110} height={90}>
				{#snippet children({ x, y })}
					<line x1={x(-60)} y1={y(-60)} x2={x(0)} y2={y(0)} class="stroke-neutral-500" stroke-width="0.6" stroke-dasharray="2 2" />
					<line
						x1={x(data.thresholdDb)}
						y1="0"
						x2={x(data.thresholdDb)}
						y2="200"
						class="stroke-amber-500"
						stroke-width="0.7"
						stroke-dasharray="2 2" />
					<polyline
						points="{x(-60)},{y(-60)} {x(data.thresholdDb)},{y(data.thresholdDb)} {x(0)},{y(bandOut(0))}"
						fill="none"
						class="stroke-signal"
						stroke-width="1.5" />
				{/snippet}
			</ParamGraph>
		</div>

		<Slider
			class="nodrag nopan nowheel"
			label="Frequency"
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
