<script lang="ts">
	import { goto } from '$app/navigation';
	import {
		useSvelteFlow,
		useUpdateNodeInternals,
		type Node,
		type NodeProps
	} from '@xyflow/svelte';
	import type { SpeakerNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
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
	const DEFAULT_SAMPLE_RATE = 48_000;
	const MIN_SAMPLE_RATE = 8_000;
	const MAX_SAMPLE_RATE = 384_000;
	const SAMPLE_RATE_PRESETS = [44_100, 48_000, 88_200, 96_000, 192_000];

	type SpeakerNodeType = Node<SpeakerNodeData, 'speaker'>;
	let { id, data }: NodeProps<SpeakerNodeType> = $props();

	const flow = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();

	let info = $state<NativeDeviceInfo | null>(null);

	let volume = $state<number | null>(null);
	// unsupported: device has no settable volume property.
	let unsupported = $state(false);

	function setDevice(value: string | null) {
		flow.updateNodeData(id, { deviceId: value, sampleRate: null });
	}

	function clampSampleRate(value: number) {
		return Math.min(
			Math.max(Math.round(value) || DEFAULT_SAMPLE_RATE, MIN_SAMPLE_RATE),
			MAX_SAMPLE_RATE
		);
	}

	function setSampleRate(value: number | null) {
		flow.updateNodeData(id, { sampleRate: value === null ? null : clampSampleRate(value) });
	}

	async function refresh() {
		await audioStore.refreshOutputDevices();
		await loadVolume();
	}

	let unlistenRefresh: (() => void) | undefined;
	onMount(() => {
		unlistenRefresh = onNodeAction(id, 'refresh', () => refresh());
	});
	onDestroy(() => unlistenRefresh?.());

	let options = $derived(audioStore.outputDevices.map((d) => ({ value: d.id, label: d.name })));
	let missing = $derived(
		!!data.deviceId && !audioStore.outputDevices.some((d) => d.id === data.deviceId)
	);

	$effect(() => {
		const deviceId = data.deviceId;
		if (!deviceId || missing) {
			info = null;
			volume = null;
			unsupported = false;
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
		void loadVolume();
		return () => {
			cancelled = true;
		};
	});

	async function loadVolume() {
		if (!data.deviceId) return;
		try {
			const v = await audioMethods.getDeviceVolume('output', data.deviceId);
			if (v === null) {
				unsupported = true;
				volume = null;
			} else {
				unsupported = false;
				volume = v;
			}
		} catch {
			unsupported = true;
			volume = null;
		}
	}

	async function setVolumePct(pct: number) {
		if (!data.deviceId || unsupported) return;
		const scalar = Math.max(0, Math.min(1, pct / 100));
		volume = scalar; // optimistic — slider stays where the user dragged it
		try {
			await audioMethods.setDeviceVolume('output', data.deviceId, scalar);
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

	let volumePct = $derived(volume === null ? 0 : volume * 100);

	let channelCount = $derived(Math.max(info?.channels ?? 2, 1));
	let effectiveSampleRate = $derived(data.sampleRate ?? info?.sampleRate ?? DEFAULT_SAMPLE_RATE);
</script>

<Wrapper label="Speaker" accent="output" icon={Speaker}>
	<div class="flex w-50 flex-col gap-1">
		<Combobox
			class="w-full"
			{options}
			value={data.deviceId ?? null}
			placeholder="— Select output —"
			onChange={setDevice}
			onOpen={() => refresh()}
		>
			{#snippet footer(close)}
				<RescanButton onRescan={refresh} />
				{#if supportsVirtualDevices}
					<ComboboxAction
						label="Add virtual device"
						icon={Add}
						onclick={() => {
							close();
							goto('/virtual-devices');
						}}
					/>
				{/if}
			{/snippet}
		</Combobox>
		{#if missing}
			<span class="text-[10px] text-red-500">Selected device not available</span>
		{:else if info}
			<span class="font-mono text-[10px] text-neutral-900">
				{formatRate(info.sampleRate)} · {info.channels} ch · {info.sampleFormat}
			</span>
			<div class="flex flex-col gap-1 text-[10px] text-neutral-900">
				<div class="flex items-center gap-1">
					<span>Sample rate</span>
					<button
						type="button"
						class={[
							'ml-auto rounded-md border px-1.5 py-0.5 text-[9px] transition-colors',
							data.sampleRate == null
								? 'border-neutral-800 bg-neutral-600 text-theme'
								: 'border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-300'
						]}
						onclick={() => setSampleRate(null)}
					>
						Auto ({formatRate(info.sampleRate)})
					</button>
				</div>
				<div class="flex items-center overflow-hidden rounded-lg border border-neutral-400 bg-neutral-100">
					<input
						class="h-7 min-w-0 flex-1 [appearance:textfield] bg-transparent text-center font-mono text-xs tabular-nums outline-none [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"
						type="number"
						min={MIN_SAMPLE_RATE}
						max={MAX_SAMPLE_RATE}
						step="1"
						value={effectiveSampleRate}
						onchange={(e) =>
							setSampleRate((e.currentTarget as HTMLInputElement).valueAsNumber)}
					/>
					<span class="border-l border-neutral-400 px-2 font-mono text-[10px]">Hz</span>
				</div>
				<div class="flex flex-wrap gap-1">
					{#each SAMPLE_RATE_PRESETS as preset (preset)}
						<button
							type="button"
							class={[
								'rounded-md border px-1.5 py-0.5 font-mono text-[9px] tabular-nums transition-colors',
								data.sampleRate === preset
									? 'border-neutral-800 bg-neutral-600 text-theme'
									: 'border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-300'
							]}
							onclick={() => setSampleRate(preset)}
						>
							{formatRate(preset)}
						</button>
					{/each}
				</div>
			</div>
		{/if}

		{#if data.deviceId && !missing}
			{#if unsupported}
				<span class="text-[10px] text-neutral-900">
					Hardware volume not adjustable for this device
				</span>
			{:else if volume !== null}
				<Slider
					label="Volume"
					value={volumePct}
					min={0}
					max={100}
					step={1}
					format={formatPct}
					ticks={[25, 50, 75]}
					onChange={setVolumePct}
				/>
			{/if}
			<InputMeter nodeId={id} side="target" {channelCount} />
		{/if}
	</div>
</Wrapper>
