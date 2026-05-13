<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { AppAudioNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import Wrapper from '../node.svelte';
	import { Combobox } from '$lib/modules/form/ui';

	type AppAudioNodeType = Node<AppAudioNodeData, 'appAudio'>;
	let { id, data }: NodeProps<AppAudioNodeType> = $props();

	const flow = useSvelteFlow();

	let refreshing = $state(false);

	function setApp(value: string | null) {
		flow.updateNodeData(id, { bundleId: value });
	}

	async function refresh() {
		refreshing = true;
		try {
			await audioStore.refreshDevices();
		} finally {
			refreshing = false;
		}
	}

	let options = $derived(
		audioStore.audioApplications.map((a) => ({
			value: a.bundleId,
			label: a.name,
			icon: a.icon ?? null
		}))
	);
	let missing = $derived(
		!!data.bundleId && !audioStore.audioApplications.some((a) => a.bundleId === data.bundleId)
	);
</script>

<Wrapper label="App Audio" accent="input" hasOutput>
	<div class="flex w-64 flex-col gap-1">
		<p class="text-[11px] text-neutral-900">
			Capture audio from a specific running app (ScreenCaptureKit, macOS 13+).
		</p>
		<div class="flex items-center gap-1">
			<Combobox {options} value={data.bundleId ?? null} placeholder="— Select application —" emptyHint="No audible apps" onChange={setApp} />
			<button
				type="button"
				class="nodrag nopan flex h-7 w-7 shrink-0 items-center justify-center rounded border border-neutral-400 bg-neutral-100 text-neutral-900 hover:bg-neutral-200 disabled:opacity-50"
				title="Refresh applications"
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
			<span class="text-[10px] text-red-500">App no longer running</span>
		{/if}
	</div>
</Wrapper>
