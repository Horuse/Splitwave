<script lang="ts">
	import { goto } from '$app/navigation';
	import { useSvelteFlow, useUpdateNodeInternals, type Node, type NodeProps } from '@xyflow/svelte';
	import type { SpeakerNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { deviceVolume } from '$lib/modules/audio/device_volume.svelte';
	import type { NativeDeviceInfo } from '$lib/modules/audio/types';
	import Wrapper from '../node.svelte';
	import InputMeter from '../input/_input_meter.svelte';
	import Slider from '../effect/_slider.svelte';
	import { Combobox, ComboboxAction, RescanButton } from '$lib/modules/form/ui';
	import { Speaker, Add } from '$lib/components/icons';
	import { onNodeAction } from '$lib/modules/flow/utils';
	import { onDestroy, onMount } from 'svelte';
	import { platform } from '@tauri-apps/plugin-os';

	const supportsVirtualDevices = platform() !== 'windows';

	type SpeakerNodeType = Node<SpeakerNodeData, 'speaker'>;
	let { id, data }: NodeProps<SpeakerNodeType> = $props();

	const flow = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();

	let info = $state<NativeDeviceInfo | null>(null);

	const volume = deviceVolume('output', () => (missing ? null : (data.deviceId ?? null)));

	function setDevice(value: string | null) {
		flow.updateNodeData(id, { deviceId: value });
	}

	async function refresh() {
		await audioStore.refreshOutputDevices();
	}

	let unlistenRefresh: (() => void) | undefined;
	onMount(() => {
		unlistenRefresh = onNodeAction(id, 'refresh', () => refresh());
	});
	onDestroy(() => unlistenRefresh?.());

	let options = $derived(audioStore.outputDevices.map((d) => ({ value: d.id, label: d.name })));
	let missing = $derived(!!data.deviceId && !audioStore.outputDevices.some((d) => d.id === data.deviceId));

	$effect(() => {
		const deviceId = data.deviceId;
		if (!deviceId || missing) {
			info = null;
			return;
		}
		let cancelled = false;
		audioMethods
			.deviceInfo('output', deviceId)
			.then((r) => {
				if (!cancelled) info = r;
			})
			.catch(() => {
				if (!cancelled) info = null;
			});
		return () => {
			cancelled = true;
		};
	});

	async function setVolumePct(pct: number) {
		await volume.set(pct / 100);
	}

	function formatRate(hz: number): string {
		return hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)} kHz` : `${hz} Hz`;
	}

	function formatPct(p: number): string {
		return `${Math.round(p)}%`;
	}

	let volumePct = $derived((volume.scalar ?? 0) * 100);
	// The graph mix is metered before the device attenuates it; without the
	// device's own dB the reading cannot be corrected to what is heard.
	let meterOffsetDb = $derived(volume.db ?? 0);

	let channelCount = $derived(Math.max(info?.channels ?? 2, 1));
</script>

<Wrapper label="Speaker" accent="output" icon={Speaker}>
	<div class="flex w-50 flex-col gap-1">
		<Combobox class="w-full" {options} value={data.deviceId ?? null} placeholder="— Select output —" onChange={setDevice} onOpen={() => refresh()}>
			{#snippet footer(close)}
				<RescanButton onRescan={refresh} />
				{#if supportsVirtualDevices}
					<ComboboxAction
						label="Add virtual device"
						icon={Add}
						onclick={() => {
							close();
							goto('/virtual-devices');
						}} />
				{/if}
			{/snippet}
		</Combobox>
		{#if missing}
			<span class="text-[10px] text-red-500">Selected device not available</span>
		{:else if info}
			<span class="font-mono text-[10px] text-neutral-900">
				{formatRate(info.sampleRate)} · {info.channels} ch · {info.sampleFormat}
			</span>
		{/if}

		{#if data.deviceId && !missing}
			{#if volume.unsupported}
				<span class="text-[10px] text-neutral-900"> Hardware volume not adjustable for this device </span>
			{:else if volume.scalar !== null}
				<Slider label="Volume" value={volumePct} min={0} max={100} step={1} format={formatPct} ticks={[25, 50, 75]} onChange={setVolumePct} />
			{/if}
			<InputMeter nodeId={id} side="target" {channelCount} dbOffset={meterOffsetDb} />
			{#if volume.scalar !== null && volume.db === null}
				<span class="text-[10px] text-neutral-900">Meter is pre-volume</span>
			{/if}
		{/if}
	</div>
</Wrapper>
