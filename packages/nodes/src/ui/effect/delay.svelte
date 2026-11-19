<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { Slider } from '@splitwave/ui';
	import { MAX_DELAY_MS } from '@splitwave/nodes/generated/engine';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { DelayNodeData } from '@splitwave/nodes';
	import ParamGraph from '../graph/param_graph.svelte';
	import Wrapper from '../node.svelte';
	import { Delay } from '../icons';
	import PresetBar from '../preset_bar.svelte';
	import type { PresetData } from '../../presets';

	type DelayNodeType = Node<DelayNodeData, 'delay'>;
	let { id, data }: NodeProps<DelayNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	function patch<K extends keyof DelayNodeData>(key: K, value: DelayNodeData[K]) {
		const p = { [key]: value } as Partial<DelayNodeData>;
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function pctFmt(v: number): string {
		return `${Math.round(v * 100)}%`;
	}

	function applyPreset(p: PresetData<'delay'>) {
		flow.updateNodeData(id, p);
		engine.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}

	// Echo taps as an impulse response: one stem per repeat, height is its level.
	const FLOOR = 0.03;

	let taps = $derived.by(() => {
		const out: { t: number; amp: number }[] = [];
		let amp = data.mix;
		let t = data.timeMs;
		while (amp > FLOOR && t <= MAX_DELAY_MS) {
			out.push({ t, amp });
			amp *= data.feedback;
			t += data.timeMs;
		}
		return out;
	});

	/** Repeats until an echo falls 30 dB under the dry signal. */
	let repeats = $derived(data.feedback <= 0 ? 1 : Math.max(1, Math.ceil(Math.log(0.0316 / Math.max(data.mix, 1e-3)) / Math.log(data.feedback)) + 1));
</script>

<Wrapper label="Delay" icon={Delay} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="flex flex-col gap-2">
		<PresetBar kind="delay" {data} onApply={applyPreset} />

		<div class="nowheel nodrag w-52">
			<ParamGraph x={[0, MAX_DELAY_MS]} y={[0, 1.1]} xTicks={[500, 1000, 1500]} yTicks={[0.5, 1]} xLabel="ms" height={72}>
				{#snippet children({ x, y })}
					<line x1={x(0)} y1={y(0)} x2={x(0)} y2={y(1 - data.mix)} class="stroke-neutral-600" stroke-width="2" />
					<circle cx={x(0)} cy={y(1 - data.mix)} r="2.5" class="fill-neutral-600" />
					{#each taps as tap, i (i)}
						<line x1={x(tap.t)} y1={y(0)} x2={x(tap.t)} y2={y(tap.amp)} class="stroke-signal" stroke-width="1.5" />
						<circle cx={x(tap.t)} cy={y(tap.amp)} r="2" class="fill-signal" />
					{/each}
				{/snippet}
				{#snippet badge()}
					<span>{Math.round(data.timeMs)} ms</span>
					<span>fb {Math.round(data.feedback * 100)}%</span>
					<span>{repeats} repeat{repeats === 1 ? '' : 's'} to -30 dB</span>
				{/snippet}
			</ParamGraph>
		</div>

		<div class="flex w-52 flex-col gap-1.5">
			<Slider
				class="nodrag nopan nowheel"
				label="Time"
				info="Time between the original sound and each echo."
				value={data.timeMs}
				min={1}
				max={MAX_DELAY_MS}
				step={1}
				unit=" ms"
				defaultValue={250}
				ticks={[100, 500, 1000]}
				onChange={(v) => patch('timeMs', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Feedback"
				info="How much of each echo is fed back to make the next one. Higher feedback means more, longer-lasting repeats."
				value={data.feedback}
				min={0}
				max={0.95}
				step={0.01}
				format={pctFmt}
				defaultValue={0.4}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('feedback', v)} />
			<Slider
				class="nodrag nopan nowheel"
				label="Mix"
				info="Blend between the original and the echoes. 0% is dry only, 100% echoes only; the original gets quieter as the mix rises."
				value={data.mix}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={0.35}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('mix', v)} />
		</div>
	</div>
</Wrapper>
