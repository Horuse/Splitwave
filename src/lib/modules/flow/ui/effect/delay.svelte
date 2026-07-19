<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { DelayNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
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

	// Echo dots — build a list of (x, height) pairs for visible taps
	const W = 208, H = 40;
	const MAX_WINDOW_MS = 2000;

	type Tap = { x: number; h: number; opacity: number };

	let taps = $derived((): Tap[] => {
		const result: Tap[] = [];
		// Dry signal at x=0
		result.push({ x: 6, h: H * 0.85, opacity: 1 });
		let amp = data.mix;
		let t = data.timeMs;
		while (amp > 0.03 && t <= MAX_WINDOW_MS) {
			const x = (t / MAX_WINDOW_MS) * (W - 12) + 6;
			result.push({ x, h: Math.max(3, amp * H * 0.85), opacity: amp });
			amp *= data.feedback;
			t += data.timeMs;
		}
		return result;
	});
</script>

<Wrapper
	label="Delay"
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex flex-col gap-2">
		<PresetBar kind="delay" {data} onApply={applyPreset} />

		<div class="nowheel nodrag">
			<svg
				width={W}
				height={H}
				class="overflow-visible rounded border border-neutral-300 bg-neutral-100 select-none"
			>
				<line x1={0} y1={H} x2={W} y2={H} stroke="currentColor" stroke-width="0.5" class="text-neutral-300" />
				{#each [500, 1000, 1500] as ms}
					{@const tx = (ms / MAX_WINDOW_MS) * (W - 12) + 6}
					<line x1={tx} y1={H - 3} x2={tx} y2={H} stroke="currentColor" stroke-width="0.5" class="text-neutral-400" />
					<text x={tx} y={H - 4} font-size="6" fill="currentColor" class="text-neutral-400" text-anchor="middle">{ms}ms</text>
				{/each}
				{#each taps() as tap, i}
					<rect
						x={tap.x - 2.5}
						y={H - tap.h}
						width={5}
						height={tap.h}
						rx="1"
						fill="#3b82f6"
						opacity={tap.opacity * (i === 0 ? 0.9 : 0.75)}
					/>
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
				onChange={(v) => patch('timeMs', v)}
			/>
			<Slider
				label="Feedback"
				value={data.feedback}
				min={0}
				max={0.95}
				step={0.01}
				format={pctFmt}
				defaultValue={0.4}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('feedback', v)}
			/>
			<Slider
				label="Mix"
				value={data.mix}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={0.35}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('mix', v)}
			/>
		</div>
	</div>
</Wrapper>
