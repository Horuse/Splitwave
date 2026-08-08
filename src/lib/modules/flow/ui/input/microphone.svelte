<script lang="ts">
	import { goto } from '$app/navigation';
	import { useSvelteFlow, useUpdateNodeInternals, type Node, type NodeProps } from '@xyflow/svelte';
	import type { MicrophoneNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { deviceVolume } from '$lib/modules/audio/device_volume.svelte';
	import type { NativeDeviceInfo } from '$lib/modules/audio/types';
	import Wrapper from '../node.svelte';
	import Slider from '../effect/_slider.svelte';
	import InputMeter from './_input_meter.svelte';
	import { Combobox, ComboboxAction, RescanButton } from '$lib/modules/form/ui';
	import { Mic, Add } from '$lib/components/icons';
	import { onNodeAction } from '$lib/modules/flow/utils';
	import { onDestroy, onMount } from 'svelte';
	import { platform } from '@tauri-apps/plugin-os';

	const supportsVirtualDevices = platform() !== 'windows';

	type MicrophoneNodeType = Node<MicrophoneNodeData, 'microphone'>;
	let { id, data }: NodeProps<MicrophoneNodeType> = $props();

	const flow = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();

	let info = $state<NativeDeviceInfo | null>(null);

	// unsupported: device has no software-settable gain (hardware-knob mics).
	const gain = deviceVolume('input', () => (missing ? null : (data.deviceId ?? null)));

	function setDevice(value: string | null) {
		flow.updateNodeData(id, { deviceId: value });
	}

	async function refresh() {
		await audioStore.refreshInputDevices();
	}

	let unlistenRefresh: (() => void) | undefined;
	onMount(() => {
		unlistenRefresh = onNodeAction(id, 'refresh', () => refresh());
	});
	onDestroy(() => unlistenRefresh?.());

	let options = $derived(audioStore.inputDevices.map((d) => ({ value: d.id, label: d.name })));
	let missing = $derived(!!data.deviceId && !audioStore.inputDevices.some((d) => d.id === data.deviceId));

	$effect(() => {
		const deviceId = data.deviceId;
		if (!deviceId || missing) {
			info = null;
			return;
		}
		let cancelled = false;
		audioMethods
			.deviceInfo('input', deviceId)
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

	async function setGainPct(pct: number) {
		await gain.set(pct / 100);
	}

	function formatRate(hz: number): string {
		return hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)} kHz` : `${hz} Hz`;
	}

	function formatPct(p: number): string {
		return `${Math.round(p)}%`;
	}

	let gainPct = $derived((gain.scalar ?? 0) * 100);

	let channelCount = $derived(Math.max(info?.channels ?? 2, 1));
</script>

<Wrapper label="Microphone" accent="input" icon={Mic}>
	<div class="flex w-50 flex-col gap-3">
		<Combobox class="w-full" {options} value={data.deviceId ?? null} placeholder="— Select microphone —" onChange={setDevice} onOpen={() => refresh()}>
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
			{#if gain.unsupported}
				<span class="text-[10px] text-neutral-900"> Input gain not adjustable for this device </span>
			{:else if gain.scalar !== null}
				<Slider label="Gain" value={gainPct} min={0} max={100} step={1} format={formatPct} ticks={[25, 50, 75]} onChange={setGainPct} />
			{/if}
			<InputMeter nodeId={id} {channelCount} />
		{/if}
	</div>
</Wrapper>
