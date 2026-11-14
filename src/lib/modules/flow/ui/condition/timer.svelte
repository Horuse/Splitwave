<script lang="ts">
	import { getContext, untrack } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { TimerMode, TimerNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import { Hourglass } from '$lib/components/icons';
	import SegmentedButtons from '$lib/components/segmented_buttons.svelte';
	import MeterBar from '$lib/components/meter_bar.svelte';
	import Slider from '../effect/_slider.svelte';
	import ControlPort from './_control_port.svelte';
	import { CONTROL_OUT, PREVIEW_CTX, START_HANDLE } from '$lib/modules/flow/utils';
	import { controlSignals, useControlInput } from '../../control.svelte';

	type TimerNodeType = Node<TimerNodeData, 'timer'>;
	let { id, data }: NodeProps<TimerNodeType> = $props();

	const flow = useSvelteFlow();
	const isPreview = getContext(PREVIEW_CTX) === true;
	const nodeId = untrack(() => id);
	const start = isPreview ? null : useControlInput(nodeId, START_HANDLE);

	const MODES: { label: string; value: TimerMode }[] = [
		{ label: 'Delay', value: 'delay' },
		{ label: 'Repeat', value: 'repeat' }
	];
	/** Long enough for any input downstream to see the rising edge. */
	const PULSE_MS = 50;

	let high = $state(false);
	let progress = $state(0);
	let running = $derived(start?.current ?? false);

	// Restarts from zero whenever start, the mode or the time changes.
	$effect(() => {
		const { mode, ms } = data;
		if (!running) {
			high = false;
			progress = 0;
			return;
		}
		let since = performance.now();
		let pulseOff: ReturnType<typeof setTimeout> | undefined;
		let frame = 0;
		const tick = (now: number) => {
			const elapsed = now - since;
			if (elapsed >= ms) {
				if (mode === 'delay') {
					high = true;
					progress = 1;
					return;
				}
				since = now;
				high = true;
				clearTimeout(pulseOff);
				pulseOff = setTimeout(() => (high = false), Math.min(PULSE_MS, ms / 2));
			}
			progress = Math.min((now - since) / ms, 1);
			frame = requestAnimationFrame(tick);
		};
		frame = requestAnimationFrame(tick);
		return () => {
			cancelAnimationFrame(frame);
			clearTimeout(pulseOff);
		};
	});

	$effect(() => {
		controlSignals.emit(nodeId, CONTROL_OUT, high);
		return () => controlSignals.clear(nodeId, CONTROL_OUT);
	});

	function set(patch: Partial<TimerNodeData>) {
		flow.updateNodeData(nodeId, patch);
	}

	function formatTime(ms: number): string {
		return ms >= 1000 ? `${(ms / 1000).toFixed(ms % 1000 === 0 ? 0 : 2)} s` : `${ms} ms`;
	}
</script>

<Wrapper label="Timer" icon={Hourglass} accent="condition">
	<div class="flex w-52 flex-col gap-3">
		<SegmentedButtons options={MODES} value={data.mode} onSelect={(mode) => set({ mode })} />
		<Slider label="Time" value={data.ms} min={50} max={10000} step={50} format={formatTime} defaultValue={1000} onChange={(ms) => set({ ms })} />
		<MeterBar class="h-1.5 rounded-sm" trackClass="bg-black/35" pct={progress * 100} fillClass={high ? 'bg-yellow-500' : 'bg-neutral-600'} />
		<div class="flex items-center justify-between">
			<ControlPort id={START_HANDLE} side="target" label="start" high={running} />
			<ControlPort id={CONTROL_OUT} side="source" label="out" {high} />
		</div>
	</div>
</Wrapper>
