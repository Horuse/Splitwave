<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { SpeakerNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import type { NativeDeviceInfo } from '$lib/modules/audio/types';
	import Wrapper from '../node.svelte';
	import Slider from '../effect/_slider.svelte';
	import { Combobox } from '$lib/modules/form/ui';

	type SpeakerNodeType = Node<SpeakerNodeData, 'speaker'>;
	let { id, data }: NodeProps<SpeakerNodeType> = $props();

	const flow = useSvelteFlow();

	let refreshing = $state(false);
	let info = $state<NativeDeviceInfo | null>(null);

	let volume = $state<number | null>(null);
	// unsupported: device has no settable volume property.
	let unsupported = $state(false);

	function setDevice(value: string | null) {
		flow.updateNodeData(id, { deviceId: value });
	}

	async function refresh() {
		refreshing = true;
		try {
			await audioStore.refreshDevices();
			await loadVolume();
		} finally {
			refreshing = false;
		}
	}

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
</script>

<Wrapper label="Speaker" accent="output" hasInput>
	<div class="flex w-50 flex-col gap-1">
		<div class="flex items-center gap-1">
			<Combobox {options} value={data.deviceId ?? null} placeholder="— Select output —" onChange={setDevice} />
			<button
				type="button"
				class="nodrag nopan flex h-7 w-7 shrink-0 items-center justify-center rounded border border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200 disabled:opacity-50"
				title="Refresh devices"
				disabled={refreshing}
				onclick={refresh}
			>
				<svg viewBox="0 0 16 16" class={['h-3.5 w-3.5', refreshing ? 'animate-spin' : '']} aria-hidden="true">
					<path
						d="M13 8a5 5 0 1 1-1.5-3.5M13 2v3h-3"
						fill="none"
						stroke="currentColor"
						stroke-width="1.5"
						stroke-linecap="round"
						stroke-linejoin="round"
					/>
				</svg>
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
		{/if}
	</div>
</Wrapper>
