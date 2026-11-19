<script lang="ts">
	import { Combobox, ComboboxAction, RescanButton, Slider } from '@splitwave/ui';
	import { useSvelteFlow, useUpdateNodeInternals, type Node, type NodeProps } from '@xyflow/svelte';
	import type { MicrophoneNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import InputMeter from './_input_meter.svelte';
	import Mic from '~icons/fluent/mic-16-filled';
	import Add from '~icons/fluent/add-24-filled';
	import { onNodeAction } from '../utils';
	import { onDestroy, onMount } from 'svelte';
	import { formatPct } from '@splitwave/nodes';
	import { getEnginePort } from '../../engine';

	const engine = getEnginePort();

	const isWindows = engine.os === 'windows';

	type MicrophoneNodeType = Node<MicrophoneNodeData, 'microphone'>;
	let { id, data }: NodeProps<MicrophoneNodeType> = $props();

	const flow = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();

	let options = $derived(engine.inputDevices.map((d) => ({ value: d.id, label: d.name })));
	let missing = $derived(!!data.deviceId && !engine.inputDevices.some((d) => d.id === data.deviceId));

	const devInfo = engine.deviceInfo({
		kind: 'input',
		deviceId: () => (missing ? null : (data.deviceId ?? null))
	});

	// unsupported: device has no software-settable gain (hardware-knob mics).
	const gain = engine.deviceVolume('input', () => (missing ? null : (data.deviceId ?? null)));

	function setDevice(value: string | null) {
		flow.updateNodeData(id, { deviceId: value });
	}

	async function refresh() {
		await engine.refreshInputDevices();
	}

	let unlistenRefresh: (() => void) | undefined;
	onMount(() => {
		unlistenRefresh = onNodeAction(id, 'refresh', () => {
			void refresh();
			void devInfo.refresh();
		});
	});
	onDestroy(() => unlistenRefresh?.());

	async function setGainPct(pct: number) {
		await gain.set(pct / 100);
	}

	let gainPct = $derived((gain.scalar ?? 0) * 100);

	let channelCount = $derived(Math.max(devInfo.channels, 1));
	let srcTooltip = $derived(devInfo.resamplingTooltip);

	$effect(() => {
		const _ = channelCount;
		updateNodeInternals(id);
	});
</script>

<Wrapper label="Microphone" accent="input" icon={Mic} {srcTooltip}>
	<div class="flex w-50 flex-col gap-2">
		<Combobox
			class="nodrag nopan w-full"
			{options}
			value={data.deviceId ?? null}
			placeholder="— Select microphone —"
			onChange={setDevice}
			onOpen={() => refresh()}>
			{#snippet footer(close)}
				<RescanButton onRescan={refresh} />
				{#if !isWindows}
					<ComboboxAction
						label="Add virtual device"
						icon={Add}
						onclick={() => {
							close();
							engine.openVirtualDevices();
						}} />
				{/if}
			{/snippet}
		</Combobox>
		{#if missing}
			<span class="text-[10px] text-red-500">Selected device not available</span>
		{:else if devInfo.info}
			<span class="node-spec">
				{devInfo.specText}
			</span>
		{:else if data.deviceId}
			<span class={['node-spec', devInfo.isLoading ? 'text-neutral-500' : 'text-red-500']}>
				{devInfo.isLoading ? 'Detecting format…' : 'Unable to detect format'}
			</span>
		{/if}

		{#if data.deviceId && !missing}
			{#if gain.unsupported}
				<span class="text-[10px] text-neutral-900"> Input gain not adjustable for this device </span>
			{:else if gain.scalar !== null}
				<Slider
					class="nodrag nopan nowheel"
					label="Gain"
					value={gainPct}
					min={0}
					max={100}
					step={1}
					format={formatPct}
					ticks={[25, 50, 75]}
					onChange={setGainPct} />
				{#if gain.unsynced}
					<span class="text-[10px] text-neutral-900">Gain changed elsewhere won't show here</span>
				{/if}
			{/if}
			<InputMeter nodeId={id} {channelCount} />
		{/if}
	</div>
</Wrapper>
