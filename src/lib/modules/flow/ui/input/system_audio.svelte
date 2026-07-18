<script lang="ts">
	import { getContext, onMount } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import type { SystemAudioNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import type { CapturePermission } from '$lib/modules/audio/types';
	import { PREVIEW_CTX, toggleGroup } from '$lib/modules/flow/utils';
	import Wrapper from '../node.svelte';
	import InputMeter from './_input_meter.svelte';
	import Slider from '../effect/_slider.svelte';
	import { SoundWave } from '$lib/components/icons';
	import { platform } from '@tauri-apps/plugin-os';

	// Self-exclusion is macOS-only; Linux (PipeWire) and Windows (WASAPI
	// loopback) need it neither.
	// Preview renders in a plain browser with no OS plugin; treat it as macOS.
	const isPreview = getContext(PREVIEW_CTX) === true;
	const isMac = isPreview || platform() === 'macos';

	type SystemAudioNodeType = Node<SystemAudioNodeData, 'systemAudio'>;
	let { id, data }: NodeProps<SystemAudioNodeType> = $props();

	const flow = useSvelteFlow();

	let permission = $state<CapturePermission | null>(null);
	let checking = $state(false);

	// Core Audio process taps have no preflight API: the grant is requested on
	// the first capture and a refusal surfaces as a pipeline error. Only the
	// ScreenCaptureKit path (macOS < 14.4) can be checked up front.
	let showBanner = $derived(
		permission !== null &&
			permission.kind === 'screenrecording' &&
			permission.state !== 'allowed'
	);

	function onToggle(e: Event) {
		const checked = (e.currentTarget as HTMLInputElement).checked;
		flow.updateNodeData(id, { excludeCurrentApp: checked });
	}

	async function refreshPermission() {
		checking = true;
		try {
			permission = await audioMethods.checkCapturePermission();
		} catch {
			permission = { kind: 'none', state: 'unknown' };
		} finally {
			checking = false;
		}
	}

	onMount(() => {
		if (isMac && !isPreview) refreshPermission();
	});

	async function openPrivacySettings() {
		try {
			await openUrl('x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture');
		} catch {
			// fall through silently -- not all hosts support deep links
		}
	}

	function setVolume(pct: number) {
		const scalar = Math.max(0, Math.min(1, pct / 100));
		flow.updateNodeData(id, { volume: scalar });
		audioMethods.setInputVolume(id, scalar).catch(() => {});
	}

	function formatPct(p: number): string {
		return `${Math.round(p)}%`;
	}

	let volumePct = $derived((data.volume ?? 1) * 100);

	// System Audio capture is stereo; expose one output handle per channel.
	const channelCount = 2;
	const expanded = true;

	function onToggleGroup(lower: number) {
		flow.updateNodeData(id, { stereoGroups: toggleGroup(data.stereoGroups ?? [], lower) });
	}
</script>

<Wrapper label="System Audio" accent="input" icon={SoundWave} hasOutput={!expanded}>
	<div class="flex w-64 flex-col gap-3">
		{#if showBanner}
			<div class={[
				'flex items-center justify-between gap-2 rounded border px-2 py-1 text-[10px]',
				permission?.state === 'denied' && 'border-red-300 bg-red-50 text-red-700',
				permission?.state === 'unknown' && 'border-neutral-300 bg-neutral-100 text-neutral-1000'
			]}>
				<span class="flex items-center gap-1.5">
					<span
						class={[
							'inline-block h-2 w-2 rounded-full',
							permission?.state === 'denied' && 'bg-red-500',
							permission?.state === 'unknown' && 'bg-neutral-500'
						]}
					></span>
					<span>
						{#if permission?.state === 'denied'}
							Screen Recording denied
						{:else}
							Checking permission…
						{/if}
					</span>
				</span>
				{#if permission?.state === 'denied'}
					<button
						type="button"
						class="nodrag nopan shrink-0 rounded border border-red-400 bg-red-100 px-1.5 py-0.5 hover:bg-red-200"
						onclick={openPrivacySettings}
					>
						Open Settings
					</button>
				{:else}
					<button
						type="button"
						class="nodrag nopan shrink-0 rounded border border-neutral-300 bg-neutral-100 px-1.5 py-0.5 hover:bg-neutral-200 disabled:opacity-50"
						title="Re-check"
						disabled={checking}
						onclick={refreshPermission}
					>
						⟳
					</button>
				{/if}
			</div>
		{/if}
		{#if isMac}
			<label class="nodrag nopan flex items-center gap-2 text-xs text-neutral-1000">
				<input
					type="checkbox"
					class="nodrag nopan rounded"
					checked={data.excludeCurrentApp ?? true}
					onchange={onToggle}
				/>
				Exclude this app (avoid feedback)
			</label>
		{/if}
		<Slider
			label="Volume"
			value={volumePct}
			min={0}
			max={100}
			step={1}
			format={formatPct}
			defaultValue={100}
			ticks={[25, 50, 75]}
			onChange={setVolume}
		/>
		<InputMeter
			nodeId={id}
			channelCount={channelCount}
			split={expanded}
			stereoGroups={data.stereoGroups ?? []}
			{onToggleGroup}
		/>
	</div>
</Wrapper>
