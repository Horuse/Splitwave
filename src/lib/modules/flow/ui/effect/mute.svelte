<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { MuteNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { SpeakerMute } from '$lib/components/icons';
	import Toggle from '$lib/components/toggle.svelte';
	import ControlPort from '../condition/_control_port.svelte';
	import { OPEN_HANDLE, PREVIEW_CTX } from '$lib/modules/flow/utils';
	import { useControlInput } from '../../control.svelte';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { Combobox, RescanButton } from '$lib/modules/form/ui';
	import { getContext, onMount, untrack } from 'svelte';
	import Slider from './_slider.svelte';
	import { formatPct } from '$lib/components/format';

	type MuteNodeType = Node<MuteNodeData, 'mute'>;
	let { id, data }: NodeProps<MuteNodeType> = $props();

	const flow = useSvelteFlow();

	const CUE_VOLUME_DEFAULT = 40;

	// Reads through the live node store rather than the `data` snapshot, which
	// a control input's effect would otherwise have captured.
	// `beep` swaps the spoken clip for a 0.2 s tone: a key driving the mute
	// toggles far too often for "muted" / "unmuted" to be usable.
	function setMuted(muted: boolean, beep = false) {
		const current = flow.getNode(id)?.data as MuteNodeData | undefined;
		if (!current || current.muted === muted) return;
		const patch = { muted };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
		if (current.cueEnabled && current.cueDeviceId) {
			audioMethods.playCue(current.cueDeviceId, muted, (current.cueVolume ?? CUE_VOLUME_DEFAULT) / 100, beep).catch(() => {});
		}
	}

	function toggle() {
		const current = flow.getNode(id)?.data as MuteNodeData | undefined;
		if (current) setMuted(!current.muted);
	}

	// A wired `open` input owns the mute: high lets the audio through, so a key
	// wired straight in is push-to-talk.
	const isPreview = getContext(PREVIEW_CTX) === true;
	const open = isPreview
		? null
		: useControlInput(
				untrack(() => id),
				OPEN_HANDLE
			);
	let wired = $derived(open?.current != null);
	$effect(() => {
		const high = open?.current;
		if (high == null) return;
		untrack(() => setMuted(!high, true));
	});

	let cueOptions = $derived(audioStore.outputDevices.map((d) => ({ value: d.id, label: d.name })));
	let cueMissing = $derived(!!data.cueDeviceId && !audioStore.outputDevices.some((d) => d.id === data.cueDeviceId));

	onMount(() => {
		if (audioStore.outputDevices.length === 0) audioStore.refreshOutputDevices();
	});

	function setCueEnabled(enabled: boolean) {
		flow.updateNodeData(id, { cueEnabled: enabled });
	}

	function setCueDevice(value: string | null) {
		flow.updateNodeData(id, { cueDeviceId: value ?? undefined });
	}

	function setCueVolume(v: number) {
		flow.updateNodeData(id, { cueVolume: v });
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
</script>

<Wrapper label="Mute" icon={SpeakerMute} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<button
		title={wired ? 'Driven by the open input' : 'Toggle mute'}
		disabled={wired}
		class={[
			'nodrag nopan flex w-40 items-center justify-center gap-2 rounded-lg border px-3 py-2 transition-colors',
			data.muted ? 'border-red-500/60 bg-red-500/10' : 'border-neutral-400 bg-neutral-100 hover:bg-neutral-200'
		]}
		onclick={toggle}>
		<span
			class={[
				'relative flex h-6 w-6 items-center justify-center rounded-full font-mono text-sm font-bold transition-colors',
				data.muted ? 'bg-red-500 text-white shadow-[0_0_8px_rgba(239,68,68,0.7)]' : 'bg-neutral-300 text-neutral-600'
			]}>
			M
			{#if data.muted}
				<span class="absolute inset-0 animate-ping rounded-full bg-red-500/40"></span>
			{/if}
		</span>
		<span class={['text-sm font-medium', data.muted ? 'text-red-500' : 'text-neutral-1100']}>
			{data.muted ? 'MUTED' : 'Active'}
		</span>
	</button>

	<div class="mt-3">
		<ControlPort id={OPEN_HANDLE} side="target" label="open" high={open?.current ?? false} />
	</div>

	<div class="nodrag nopan mt-3 flex w-40 flex-col gap-1 border-t border-neutral-400 pt-2">
		<Toggle size="sm" checked={data.cueEnabled ?? false} label="Sound cue" onChange={setCueEnabled} />

		{#if data.cueEnabled}
			<span class="text-[10px] text-neutral-900">Plays on the output device</span>
			<Combobox class="w-full" options={cueOptions} value={data.cueDeviceId ?? null} placeholder="— Select output —" onChange={setCueDevice}>
				{#snippet footer(close)}
					<RescanButton
						onRescan={() => {
							close();
							audioStore.refreshOutputDevices();
						}} />
				{/snippet}
			</Combobox>
			{#if cueMissing}
				<span class="text-[10px] text-red-500">Selected device not available</span>
			{/if}
			<Slider
				label="Cue volume"
				value={data.cueVolume ?? CUE_VOLUME_DEFAULT}
				min={0}
				max={100}
				step={1}
				format={formatPct}
				defaultValue={CUE_VOLUME_DEFAULT}
				ticks={[25, 50, 75]}
				onChange={setCueVolume} />
		{/if}
	</div>
</Wrapper>
