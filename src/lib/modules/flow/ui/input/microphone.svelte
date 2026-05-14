<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { MicrophoneNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import type { NativeDeviceInfo } from '$lib/modules/audio/types';
	import Wrapper from '../node.svelte';
	import Slider from '../effect/_slider.svelte';
	import InputMeter from './_input_meter.svelte';
	import { Combobox } from '$lib/modules/form/ui';
	import { Refresh } from '$lib/components/icons';
	import { onNodeAction } from '$lib/modules/flow/utils';
	import { onDestroy, onMount } from 'svelte';

	type MicrophoneNodeType = Node<MicrophoneNodeData, 'microphone'>;
	let { id, data }: NodeProps<MicrophoneNodeType> = $props();

	const flow = useSvelteFlow();

	let refreshing = $state(false);
	let info = $state<NativeDeviceInfo | null>(null);

	let gain = $state<number | null>(null);
	// unsupported: device has no software-settable gain (hardware-knob mics).
	let unsupported = $state(false);

	function setDevice(value: string | null) {
		flow.updateNodeData(id, { deviceId: value });
	}

	async function refresh() {
		refreshing = true;
		try {
			await audioStore.refreshInputDevices();
			await loadGain();
		} finally {
			refreshing = false;
		}
	}

	let unlistenRefresh: (() => void) | undefined;
	onMount(() => {
		unlistenRefresh = onNodeAction(id, 'refresh', () => refresh());
	});
	onDestroy(() => unlistenRefresh?.());

	let options = $derived(audioStore.inputDevices.map((d) => ({ value: d.id, label: d.name })));
	let missing = $derived(
		!!data.deviceId && !audioStore.inputDevices.some((d) => d.id === data.deviceId)
	);

	$effect(() => {
		const deviceId = data.deviceId;
		if (!deviceId || missing) {
			info = null;
			gain = null;
			unsupported = false;
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
		void loadGain();
		return () => {
			cancelled = true;
		};
	});

	async function loadGain() {
		if (!data.deviceId) return;
		try {
			const v = await audioMethods.getDeviceVolume('input', data.deviceId);
			if (v === null) {
				unsupported = true;
				gain = null;
			} else {
				unsupported = false;
				gain = v;
			}
		} catch {
			unsupported = true;
			gain = null;
		}
	}

	async function setGainPct(pct: number) {
		if (!data.deviceId || unsupported) return;
		const scalar = Math.max(0, Math.min(1, pct / 100));
		gain = scalar;
		try {
			await audioMethods.setDeviceVolume('input', data.deviceId, scalar);
		} catch {
			unsupported = true;
		}
	}

	function formatRate(hz: number): string {
		return hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 === 0 ? 0 : 1)} kHz` : `${hz} Hz`;
	}

	function formatPct(p: number): string {
		return `${Math.round(p)}%`;
	}

	let gainPct = $derived(gain === null ? 0 : gain * 100);
</script>

<Wrapper label="Microphone" accent="input" hasOutput>
	<div class="flex w-50 flex-col gap-1">
		<div class="flex items-center gap-1">
			<Combobox {options} value={data.deviceId ?? null} placeholder="— Select microphone —" onChange={setDevice} />
			<button
				type="button"
				class="nodrag nopan flex h-7 w-7 shrink-0 items-center justify-center rounded border border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200 disabled:opacity-50"
				title="Refresh devices"
				disabled={refreshing}
				onclick={refresh}
			>
				<Refresh class={['h-3.5 w-3.5', refreshing ? 'animate-spin' : '']} />
			</button>
		</div>
		{#if missing}
			<span class="text-[10px] text-red-500">Selected device not available</span>
		{:else if info}
			<span class="font-mono text-[10px] text-neutral-900">
				{formatRate(info.sampleRate)} · {info.channels} ch · {info.sampleFormat}
			</span>
		{/if}

		{#if data.deviceId && !missing}
			{#if unsupported}
				<span class="text-[10px] text-neutral-900">
					Input gain not adjustable for this device
				</span>
			{:else if gain !== null}
				<Slider
					label="Gain"
					value={gainPct}
					min={0}
					max={100}
					step={1}
					format={formatPct}
					ticks={[25, 50, 75]}
					onChange={setGainPct}
				/>
			{/if}
			<InputMeter nodeId={id} />
		{/if}
	</div>
</Wrapper>
