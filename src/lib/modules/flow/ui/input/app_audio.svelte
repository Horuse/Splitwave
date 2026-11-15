<script lang="ts">
	import { useSvelteFlow, useUpdateNodeInternals, type Node, type NodeProps } from '@xyflow/svelte';
	import type { AppAudioNodeData } from '$lib/modules/pipeline/types';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { useDeviceInfo } from '$lib/modules/audio';
	import Wrapper from '../node.svelte';
	import InputMeter from './_input_meter.svelte';
	import Slider from '../effect/_slider.svelte';
	import { Combobox, RescanButton } from '$lib/modules/form/ui';
	import { Apps } from '$lib/components/icons';
	import { onNodeAction } from '$lib/modules/flow/utils';
	import { onDestroy, onMount } from 'svelte';
	import { formatPct } from '$lib/components/format';
	import { t } from '$lib/modules/i18n';

	type AppAudioNodeType = Node<AppAudioNodeData, 'appAudio'>;
	let { id, data }: NodeProps<AppAudioNodeType> = $props();

	const flow = useSvelteFlow();
	const updateNodeInternals = useUpdateNodeInternals();

	const devInfo = useDeviceInfo({ kind: 'app' });

	function setApp(value: string | null) {
		flow.updateNodeData(id, { bundleId: value });
	}

	async function refresh() {
		await audioStore.refreshAudioApplications();
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
		audioStore.audioApplications.map((a) => ({
			value: a.bundleId,
			label: a.name,
			icon: a.icon ?? null
		}))
	);
	let missing = $derived(!!data.bundleId && !audioStore.audioApplications.some((a) => a.bundleId === data.bundleId));

	function setVolume(pct: number) {
		const scalar = Math.max(0, Math.min(1, pct / 100));
		flow.updateNodeData(id, { volume: scalar });
		audioMethods.setInputVolume(id, scalar).catch(() => {});
	}

	let volumePct = $derived((data.volume ?? 1) * 100);

	let channelCount = $derived(devInfo.channels);
	let srcTooltip = $derived(devInfo.resamplingTooltip);

	$effect(() => {
		const _ = channelCount;
		updateNodeInternals(id);
	});
</script>

<Wrapper label={t('node.appAudio.label')} accent="input" icon={Apps} {srcTooltip}>
	<div class="flex w-64 flex-col gap-2">
		<Combobox
			class="w-full"
			{options}
			value={data.bundleId ?? null}
			placeholder={t('node.appAudio.select')}
			emptyHint={t('node.appAudio.noApps')}
			onChange={setApp}
			onOpen={() => refresh()}>
			{#snippet footer()}
				<RescanButton onRescan={refresh} />
			{/snippet}
		</Combobox>
		{#if missing}
			<span class="text-[10px] text-red-500">{t('node.appAudio.notRunning')}</span>
		{:else if data.bundleId && devInfo.info}
			<span class="node-spec">{devInfo.specText}</span>
		{:else if data.bundleId}
			<span class={['node-spec', devInfo.isLoading ? 'text-neutral-500' : 'text-red-500']}>
				{devInfo.isLoading ? t('node.device.detecting') : t('node.device.detectFailed')}
			</span>
		{/if}
		<Slider
			label={t('node.slider.volume')}
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
