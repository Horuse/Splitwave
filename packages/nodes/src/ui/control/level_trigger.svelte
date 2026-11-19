<script lang="ts">
	import { Slider } from '@splitwave/ui';
	import { untrack } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { LevelTriggerNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import Gauge from '~icons/fluent/gauge-16-filled';
	import ControlPort from '../_control_port.svelte';
	import { CONTROL_OUT } from '../utils';
	import { controlSignals } from '../state/control.svelte';
	import { getEnginePort } from '../../engine';

	const engine = getEnginePort();

	type LevelTriggerNodeType = Node<LevelTriggerNodeData, 'levelTrigger'>;
	let { id, data }: NodeProps<LevelTriggerNodeType> = $props();

	const flow = useSvelteFlow();
	const nodeId = untrack(() => id);

	const DB_FLOOR = -60;

	let levelDb = $state(-Infinity);
	let high = $state(false);
	let lastAbove = 0;

	function toPct(db: number): number {
		if (!isFinite(db)) return 0;
		return Math.max(0, Math.min(100, ((db - DB_FLOOR) / -DB_FLOOR) * 100));
	}

	// The engine meters this node's input on each tick; the gate opens at the
	// threshold and closes only once the level has stayed under it for `holdMs`,
	// so the gaps between words do not flicker it.
	engine.listen<{ nodeId: string; peaks: number[] }>('audio://meter', (p) => {
		if (p.nodeId !== nodeId) return;
		const peak = p.peaks.length > 0 ? Math.max(...p.peaks) : 0;
		levelDb = peak <= 1e-6 ? -Infinity : 20 * Math.log10(peak);
		const now = performance.now();
		if (levelDb >= data.thresholdDb) {
			lastAbove = now;
			high = true;
		} else if (now - lastAbove > data.holdMs) {
			high = false;
		}
	});

	$effect(() => {
		controlSignals.emit(nodeId, CONTROL_OUT, high);
		return () => controlSignals.clear(nodeId, CONTROL_OUT);
	});

	function set(patch: Partial<LevelTriggerNodeData>) {
		flow.updateNodeData(nodeId, patch);
	}
</script>

<Wrapper label="Level Trigger" icon={Gauge} accent="control" hasInput channelIo nodeId={id}>
	<div class="flex w-48 flex-col gap-2">
		<!-- Input level against the threshold; the fill lights up while the output is high. -->
		<div class="flex flex-col gap-1">
			<div class="relative h-1.5 rounded-full bg-(--lv-1)">
				<div class={['absolute inset-y-0 left-0 rounded-full', high ? 'bg-signal' : 'bg-neutral-600']} style="width: {toPct(levelDb)}%;"></div>
				<span
					class="pointer-events-none absolute -top-1 -bottom-1 w-0.5 -translate-x-1/2 rounded-full bg-neutral-1100"
					style="left: {toPct(data.thresholdDb)}%"></span>
			</div>
			<div class="flex justify-between font-mono text-[9px] text-neutral-600 tabular-nums">
				<span>{DB_FLOOR}</span>
				<span class={high ? 'text-signal' : 'text-neutral-900'}>{isFinite(levelDb) ? `${levelDb.toFixed(1)} dB` : '-inf'}</span>
				<span>0</span>
			</div>
		</div>
		<Slider
			class="nodrag nopan nowheel"
			label="Threshold"
			info="Output goes high while the input is louder than this level."
			value={data.thresholdDb}
			min={DB_FLOOR}
			max={0}
			step={1}
			unit="dB"
			defaultValue={-40}
			onChange={(v) => set({ thresholdDb: v })} />
		<Slider
			class="nodrag nopan nowheel"
			label="Hold"
			info="How long the output stays high after the input drops below the threshold, so speech does not flicker it."
			value={data.holdMs}
			min={0}
			max={2000}
			step={10}
			unit="ms"
			defaultValue={300}
			onChange={(v) => set({ holdMs: v })} />
		<ControlPort id={CONTROL_OUT} side="source" label="out" {high} />
	</div>
</Wrapper>
