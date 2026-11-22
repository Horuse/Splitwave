<script lang="ts">
	import { Button, Slider, Toggle } from '@splitwave/ui';
	import { getContext, onMount } from 'svelte';
	import { useSvelteFlow, useUpdateNodeInternals, type Node, type NodeProps } from '@xyflow/svelte';
	import type { SystemAudioNodeData } from '@splitwave/nodes';
	import { PREVIEW_CTX } from '../utils';
	import Wrapper from '../node.svelte';
	import InputMeter from './_input_meter.svelte';
	import SoundWave from '~icons/fluent/sound-wave-circle-16-filled';
	import { formatPct } from '@splitwave/nodes';
	import { getEnginePort, type CapturePermission } from '../../engine';

	const engine = getEnginePort();

	// Self-exclusion is macOS-only; Linux (PipeWire) and Windows (WASAPI
	// loopback) need it neither.
	// Preview renders in a plain browser with no OS plugin; treat it as macOS.
	const isPreview = getContext(PREVIEW_CTX) === true;
	const isMac = isPreview || engine.os === 'macos';

	type SystemAudioNodeType = Node<SystemAudioNodeData, 'systemAudio'>;
	let { id, data }: NodeProps<SystemAudioNodeType> = $props();

	const flow = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();

	const devInfo = engine.deviceInfo({ kind: 'system' });

	let permission = $state<CapturePermission | null>(null);
	let checking = $state(false);

	// Core Audio process taps have no preflight API: the grant is requested on
	// the first capture and a refusal surfaces as a pipeline error. Only the
	// ScreenCaptureKit path (macOS < 14.4) can be checked up front.
	let showBanner = $derived(permission !== null && permission.kind === 'screenrecording' && permission.state !== 'allowed');

	async function refreshPermission() {
		checking = true;
		try {
			permission = await engine.audio.checkCapturePermission();
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
			await engine.openUrl('x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture');
		} catch {
			// fall through silently -- not all hosts support deep links
		}
	}

	function setVolume(pct: number) {
		const scalar = Math.max(0, Math.min(1, pct / 100));
		flow.updateNodeData(id, { volume: scalar });
		engine.audio.setInputVolume(id, scalar).catch(() => {});
	}

	let volumePct = $derived((data.volume ?? 1) * 100);

	let channelCount = $derived(devInfo.channels);
	let srcTooltip = $derived(devInfo.resamplingTooltip);

	$effect(() => {
		const _ = channelCount;
		updateNodeInternals(id);
	});
</script>

<Wrapper label="System Audio" accent="input" icon={SoundWave} {srcTooltip}>
	<div class="flex w-64 flex-col gap-2">
		{#if showBanner}
			<div
				class={[
					'flex items-center justify-between gap-2 rounded-lg py-1 pr-1 pl-2.5 text-[10px]',
					permission?.state === 'denied' ? 'bg-red-500/10 text-red-500' : 'bg-(--field-bg) text-neutral-900'
				]}>
				<span>{permission?.state === 'denied' ? 'Screen Recording denied' : 'Checking permission...'}</span>
				{#if permission?.state === 'denied'}
					<Button class="nodrag nopan" size="xs" tone="danger" onclick={openPrivacySettings}>Open Settings</Button>
				{:else}
					<Button class="nodrag nopan" size="xs" disabled={checking} onclick={refreshPermission}>Re-check</Button>
				{/if}
			</div>
		{/if}
		{#if isMac}
			<Toggle
				class="nodrag nopan"
				size="sm"
				label="Exclude this app"
				checked={data.excludeCurrentApp ?? true}
				onChange={(v) => flow.updateNodeData(id, { excludeCurrentApp: v })} />
		{/if}
		{#if devInfo.info}
			<span class="node-spec">{devInfo.specText}</span>
		{:else}
			<span class={['node-spec', devInfo.isLoading ? 'text-neutral-500' : 'text-red-500']}>
				{devInfo.isLoading ? 'Detecting format…' : 'Unable to detect format'}
			</span>
		{/if}
		<Slider
			class="nodrag nopan nowheel"
			label="Volume"
			info="Level of the captured system mix as it enters the pipeline. Does not change what you hear from the system itself."
			value={volumePct}
			min={0}
			max={100}
			step={1}
			format={formatPct}
			defaultValue={100}
			ticks={[25, 50, 75]}
			onChange={setVolume} />
		<InputMeter nodeId={id} {channelCount} />
	</div>
</Wrapper>
