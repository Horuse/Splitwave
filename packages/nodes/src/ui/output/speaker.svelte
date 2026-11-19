<script lang="ts">
	import { Combobox, ComboboxAction, RescanButton, Slider } from '@splitwave/ui';
	import { useSvelteFlow, useUpdateNodeInternals, type Node, type NodeProps } from '@xyflow/svelte';
	import type { SpeakerNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import InputMeter from '../input/_input_meter.svelte';
	import Speaker from '~icons/fluent/speaker-2-16-filled';
	import Add from '~icons/fluent/add-24-filled';
	import { onNodeAction } from '../utils';
	import { onDestroy, onMount } from 'svelte';
	import { formatPct } from '@splitwave/nodes';
	import { getEnginePort } from '../../engine';

	const engine = getEnginePort();

	const isWindows = engine.os === 'windows';
	const virtualDevicesLabel = isWindows ? 'Use virtual microphone' : 'Add virtual device';

	type SpeakerNodeType = Node<SpeakerNodeData, 'speaker'>;
	let { id, data }: NodeProps<SpeakerNodeType> = $props();

	const flow = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();

	let options = $derived(engine.outputDevices.map((d) => ({ value: d.id, label: d.name })));
	let missing = $derived(!!data.deviceId && !engine.outputDevices.some((d) => d.id === data.deviceId));

	const devInfo = engine.deviceInfo({
		kind: 'output',
		deviceId: () => (missing ? null : (data.deviceId ?? null))
	});

	const volume = engine.deviceVolume('output', () => (missing ? null : (data.deviceId ?? null)));

	function setDevice(value: string | null) {
		flow.updateNodeData(id, { deviceId: value });
	}

	async function refresh() {
		await engine.refreshOutputDevices();
	}

	async function openVirtualDevice() {
		await engine.openVirtualDevices();
	}

	let unlistenRefresh: (() => void) | undefined;
	onMount(() => {
		unlistenRefresh = onNodeAction(id, 'refresh', () => {
			void refresh();
			void devInfo.refresh();
		});
	});
	onDestroy(() => unlistenRefresh?.());

	async function setVolumePct(pct: number) {
		await volume.set(pct / 100);
	}

	let volumePct = $derived((volume.scalar ?? 0) * 100);
	// The graph mix is metered before the device attenuates it; without the
	// device's own dB the reading cannot be corrected to what is heard.
	let meterOffsetDb = $derived(volume.db ?? 0);

	let channelCount = $derived(Math.max(devInfo.channels, 1));
	let srcTooltip = $derived(devInfo.resamplingTooltip);

	$effect(() => {
		const _ = channelCount;
		updateNodeInternals(id);
	});
</script>

<Wrapper label="Speaker" accent="output" icon={Speaker} {srcTooltip}>
	<div class="flex w-50 flex-col gap-1">
		<Combobox
			class="nodrag nopan w-full"
			{options}
			value={data.deviceId ?? null}
			placeholder="— Select output —"
			onChange={setDevice}
			onOpen={() => refresh()}>
			{#snippet footer(close)}
				<RescanButton onRescan={refresh} />
				<ComboboxAction
					label={virtualDevicesLabel}
					icon={Add}
					onclick={() => {
						close();
						void openVirtualDevice();
					}} />
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
			{#if volume.unsupported}
				<span class="text-[10px] text-neutral-900"> Hardware volume not adjustable for this device </span>
			{:else if volume.scalar !== null}
				<Slider
					class="nodrag nopan nowheel"
					label="Volume"
					value={volumePct}
					min={0}
					max={100}
					step={1}
					format={formatPct}
					ticks={[25, 50, 75]}
					onChange={setVolumePct} />
				{#if volume.unsynced}
					<span class="text-[10px] text-neutral-900">Volume changed elsewhere won't show here</span>
				{/if}
			{/if}
			<InputMeter nodeId={id} side="target" {channelCount} dbOffset={meterOffsetDb} />
			{#if volume.scalar !== null && volume.db === null}
				<span class="text-[10px] text-neutral-900">Meter is pre-volume</span>
			{/if}
		{/if}
	</div>
</Wrapper>
