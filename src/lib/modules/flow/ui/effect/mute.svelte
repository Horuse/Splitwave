<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { MuteNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { SpeakerMute, Dismiss } from '$lib/components/icons';
	import Toggle from '$lib/components/toggle.svelte';
	import { register, unregister } from '@tauri-apps/plugin-global-shortcut';
	import { accelerator, formatAccelerator } from './_accelerator';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { Combobox, RescanButton } from '$lib/modules/form/ui';
	import { onMount } from 'svelte';
	import Slider from './_slider.svelte';

	type MuteNodeType = Node<MuteNodeData, 'mute'>;
	let { id, data }: NodeProps<MuteNodeType> = $props();

	const flow = useSvelteFlow();

	const CUE_VOLUME_DEFAULT = 40;

	let binding = $state(false);
	let bindError = $state('');

	// Reads through the live node store rather than the `data` snapshot: the
	// shortcut handler is registered once per accelerator and would otherwise
	// keep toggling off a value captured at registration time.
	function toggle() {
		const current = flow.getNode(id)?.data as MuteNodeData | undefined;
		if (!current) return;
		const patch = { muted: !current.muted };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
		if (current.cueEnabled && current.cueDeviceId) {
			audioMethods
				.playCue(current.cueDeviceId, patch.muted, (current.cueVolume ?? CUE_VOLUME_DEFAULT) / 100)
				.catch(() => {});
		}
	}

	$effect(() => {
		const hotkey = data.hotkey;
		if (!hotkey) return;

		let live = true;
		// A leftover registration from a previous mount (HMR, node re-render)
		// makes `register` fail with "already registered"; clear it first.
		unregister(hotkey)
			.catch(() => {})
			.then(() => {
				if (!live) return;
				return register(hotkey, (e) => {
					if (live && e.state === 'Pressed') toggle();
				});
			})
			.then(() => {
				if (live) bindError = '';
			})
			.catch((err) => {
				if (live) bindError = String(err);
			});

		return () => {
			live = false;
			unregister(hotkey).catch(() => {});
		};
	});

	function capture(e: KeyboardEvent) {
		e.preventDefault();
		if (e.key === 'Escape') {
			binding = false;
			return;
		}
		const combo = accelerator(e);
		if (!combo) return;
		bindError = '';
		binding = false;
		flow.updateNodeData(id, { hotkey: combo });
	}

	let cueOptions = $derived(audioStore.outputDevices.map((d) => ({ value: d.id, label: d.name })));
	let cueMissing = $derived(
		!!data.cueDeviceId && !audioStore.outputDevices.some((d) => d.id === data.cueDeviceId)
	);

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

	function formatPct(v: number): string {
		return `${Math.round(v)}%`;
	}

	function clearHotkey() {
		bindError = '';
		flow.updateNodeData(id, { hotkey: undefined });
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
</script>

<Wrapper
	label="Mute"
	icon={SpeakerMute}
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<button
		title="Toggle mute (M)"
		class={[
			'nodrag nopan flex w-40 items-center justify-center gap-2 rounded-lg border px-3 py-2 transition-colors',
			data.muted
				? 'border-red-500/60 bg-red-500/10'
				: 'border-neutral-400 bg-neutral-100 hover:bg-neutral-200'
		]}
		onclick={toggle}
	>
		<span
			class={[
				'relative flex h-6 w-6 items-center justify-center rounded-full font-mono text-sm font-bold transition-colors',
				data.muted
					? 'bg-red-500 text-white shadow-[0_0_8px_rgba(239,68,68,0.7)]'
					: 'bg-neutral-300 text-neutral-600'
			]}
		>
			M
			{#if data.muted}
				<span class="absolute inset-0 animate-ping rounded-full bg-red-500/40"></span>
			{/if}
		</span>
		<span class={[
			'text-sm font-medium',
			data.muted ? 'text-red-500' : 'text-neutral-1100'
		]}>
			{data.muted ? 'MUTED' : 'Active'}
		</span>
	</button>

	<div class="nodrag nopan mt-3 flex w-40 items-center gap-1">
		<button
			title={binding ? 'Press any key, Esc to cancel' : 'Bind a global shortcut'}
			class={[
				'flex-1 truncate rounded-md border px-2 py-1 font-mono text-xs tabular-nums transition-colors',
				binding
					? 'border-blue-500/60 bg-blue-500/10 text-blue-500'
					: 'border-neutral-400 bg-neutral-100 text-neutral-1100 hover:bg-neutral-200'
			]}
			onclick={() => (binding = !binding)}
		>
			{binding ? 'Press a key...' : data.hotkey ? formatAccelerator(data.hotkey) : 'Bind key'}
		</button>
		{#if data.hotkey && !binding}
			<button
				title="Clear shortcut"
				class="rounded-md border border-neutral-400 bg-neutral-100 p-1 text-neutral-900 hover:bg-neutral-200"
				onclick={clearHotkey}
			>
				<Dismiss class="size-3" />
			</button>
		{/if}
	</div>

	{#if bindError}
		<p class="mt-1 w-40 text-[10px] break-words text-red-500">{bindError}</p>
	{/if}

	<div class="nodrag nopan mt-3 flex w-40 flex-col gap-1 border-t border-neutral-400 pt-2">
		<Toggle
			size="sm"
			checked={data.cueEnabled ?? false}
			label="Sound cue"
			onChange={setCueEnabled}
		/>

		{#if data.cueEnabled}
			<span class="text-[10px] text-neutral-900">Plays on the output device</span>
			<Combobox
				class="w-full"
				options={cueOptions}
				value={data.cueDeviceId ?? null}
				placeholder="— Select output —"
				onChange={setCueDevice}
			>
				{#snippet footer(close)}
					<RescanButton
						onRescan={() => {
							close();
							audioStore.refreshOutputDevices();
						}}
					/>
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
				onChange={setCueVolume}
			/>
		{/if}
	</div>
</Wrapper>

<svelte:window onkeydown={binding ? capture : undefined} />
