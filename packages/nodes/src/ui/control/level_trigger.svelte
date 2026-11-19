<script lang="ts">
	import { MeterBar, Slider } from '@splitwave/ui';
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
		<div class="relative">
			<MeterBar class="h-2 rounded-sm" pct={toPct(levelDb)} fillClass={high ? 'bg-yellow-500' : 'bg-neutral-600'} />
			<!-- Threshold marker -->
			<span class="pointer-events-none absolute -top-0.5 -bottom-0.5 w-px bg-neutral-1100" style="left:{toPct(data.thresholdDb)}%"></span>
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
