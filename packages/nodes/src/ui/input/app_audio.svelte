<script lang="ts">
	import { Combobox, RescanButton, Slider } from '@splitwave/ui';
	import { useSvelteFlow, useUpdateNodeInternals, type Node, type NodeProps } from '@xyflow/svelte';
	import type { AppAudioNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import InputMeter from './_input_meter.svelte';
	import Apps from '~icons/fluent/apps-16-filled';
	import { onNodeAction } from '../utils';
	import { onDestroy, onMount } from 'svelte';
	import { formatPct } from '@splitwave/nodes';
	import { getEnginePort } from '../../engine';

	const engine = getEnginePort();

	type AppAudioNodeType = Node<AppAudioNodeData, 'appAudio'>;
	let { id, data }: NodeProps<AppAudioNodeType> = $props();

	const flow = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();

	const devInfo = engine.deviceInfo({ kind: 'app' });

	function setApp(value: string | null) {
		flow.updateNodeData(id, { bundleId: value });
	}

	async function refresh() {
		await engine.refreshAudioApplications();
	}

	let unlistenRefresh: (() => void) | undefined;
	onMount(() => {
		unlistenRefresh = onNodeAction(id, 'refresh', () => {
			void refresh();
			void devInfo.refresh();
		});
	});
	onDestroy(() => {
		unlistenRefresh?.();
	});

	let options = $derived(
		engine.audioApplications.map((a) => ({
			value: a.bundleId,
			label: a.name,
			icon: a.icon ?? null
		}))
	);
	let missing = $derived(!!data.bundleId && !engine.audioApplications.some((a) => a.bundleId === data.bundleId));

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

<Wrapper label="App Audio" accent="input" icon={Apps} {srcTooltip}>
	<div class="flex w-64 flex-col gap-2">
		<Combobox
			class="w-full"
			{options}
			value={data.bundleId ?? null}
			placeholder="— Select application —"
			emptyHint="No audible apps"
			onChange={setApp}
			onOpen={() => refresh()}>
			{#snippet footer()}
				<RescanButton onRescan={refresh} />
			{/snippet}
		</Combobox>
		{#if missing}
			<span class="text-[10px] text-red-500">App no longer running</span>
		{:else if data.bundleId && devInfo.info}
			<span class="node-spec">{devInfo.specText}</span>
		{:else if data.bundleId}
			<span class={['node-spec', devInfo.isLoading ? 'text-neutral-500' : 'text-red-500']}>
				{devInfo.isLoading ? 'Detecting format…' : 'Unable to detect format'}
			</span>
		{/if}
		<Slider
			class="nodrag nopan nowheel"
			label="Volume"
			info="Level of the captured app as it enters the pipeline. The app keeps playing at its own volume."
			value={volumePct}
			min={0}
			max={100}
			step={1}
			format={formatPct}
			defaultValue={100}
			ticks={[25, 50, 75]}
			onChange={setVolume} />
		{#if data.bundleId && !missing}
			<InputMeter nodeId={id} {channelCount} />
		{/if}
	</div>
</Wrapper>
