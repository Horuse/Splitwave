<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { DelayNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { Delay } from '$lib/components/icons';
	import { PresetBar } from '$lib/modules/preset/ui';
	import type { PresetData } from '$lib/modules/preset';
	import Slider from './_slider.svelte';

	type DelayNodeType = Node<DelayNodeData, 'delay'>;
	let { id, data }: NodeProps<DelayNodeType> = $props();

	const flow = useSvelteFlow();

	function patch<K extends keyof DelayNodeData>(key: K, value: DelayNodeData[K]) {
		const p = { [key]: value } as Partial<DelayNodeData>;
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function pctFmt(v: number): string {
		return `${Math.round(v * 100)}%`;
	}

	function applyPreset(p: PresetData<'delay'>) {
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}

	// Echo taps as an impulse response — one stem per tap, height = amplitude.
	const W = 208,
		H = 64;
	const PAD = 10;
	const TOP = 8;
	const BASE = H - 16; // baseline sits above a band reserved for time labels
	const MAX_WINDOW_MS = 2000;

	type Tap = { x: number; h: number; opacity: number; dry: boolean };

	let taps = $derived((): Tap[] => {
		const result: Tap[] = [];
		result.push({ x: PAD, h: BASE - TOP, opacity: 1, dry: true });
		let amp = data.mix;
		let t = data.timeMs;
		while (amp > 0.03 && t <= MAX_WINDOW_MS) {
			const x = (t / MAX_WINDOW_MS) * (W - PAD * 2) + PAD;
			result.push({ x, h: Math.max(2, amp * (BASE - TOP)), opacity: amp, dry: false });
			amp *= data.feedback;
			t += data.timeMs;
		}
		return result;
	});
</script>

<Wrapper label="Delay" icon={Delay} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="flex flex-col gap-2">
		<PresetBar kind="delay" {data} onApply={applyPreset} />

		<div class="nowheel nodrag">
			<svg width={W} height={H} class="rounded border border-neutral-300 bg-neutral-100 select-none">
				<line x1={PAD - 10} y1={BASE} x2={W - PAD + 8} y2={BASE} stroke="currentColor" stroke-width="0.75" class="text-neutral-300" />
				{#each [500, 1000, 1500] as ms}
					{@const tx = (ms / MAX_WINDOW_MS) * (W - PAD * 2) + PAD}
					<line x1={tx} y1={BASE} x2={tx} y2={BASE + 3} stroke="currentColor" stroke-width="0.5" class="text-neutral-400" />
					<text x={tx} y={H - 5} font-size="7" fill="currentColor" class="text-neutral-500" text-anchor="middle">{ms}ms</text>
				{/each}
				{#each taps() as tap, i (i)}
					<line
						x1={tap.x}
						y1={BASE}
						x2={tap.x}
						y2={BASE - tap.h}
						stroke={tap.dry ? 'currentColor' : '#f59e0b'}
						class={tap.dry ? 'text-neutral-500' : ''}
						stroke-width={tap.dry ? 2 : 1.5}
						opacity={tap.dry ? 1 : Math.max(0.5, tap.opacity)} />
					<circle
						cx={tap.x}
						cy={BASE - tap.h}
						r={tap.dry ? 2.5 : 1.8}
						fill={tap.dry ? 'currentColor' : '#f59e0b'}
						class={tap.dry ? 'text-neutral-500' : ''}
						opacity={tap.dry ? 1 : Math.max(0.5, tap.opacity)} />
				{/each}
			</svg>
		</div>

		<div class="flex w-52 flex-col gap-1.5">
			<Slider
				label="Time"
				value={data.timeMs}
				min={1}
				max={2000}
				step={1}
				unit=" ms"
				defaultValue={250}
				ticks={[100, 500, 1000]}
				onChange={(v) => patch('timeMs', v)} />
			<Slider
				label="Feedback"
				value={data.feedback}
				min={0}
				max={0.95}
				step={0.01}
				format={pctFmt}
				defaultValue={0.4}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('feedback', v)} />
			<Slider
				label="Mix"
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
