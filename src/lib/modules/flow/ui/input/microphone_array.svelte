<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import { onDestroy, onMount } from 'svelte';
	import toast from 'svelte-french-toast';
	import { Mic } from '$lib/components/icons';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import { audioStore } from '$lib/modules/audio/stores.svelte';
	import type { MicrophoneArrayMetrics } from '$lib/modules/audio/types';
	import { modalManager } from '$lib/modules/overlay/modal';
	import type { MicrophoneArrayNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';
	import Setup from './_microphone_array_setup.svelte';

	type MicrophoneArrayNodeType = Node<MicrophoneArrayNodeData, 'microphoneArray'>;
	let { id, data }: NodeProps<MicrophoneArrayNodeType> = $props();

	const flow = useSvelteFlow();
	type SetupParams = {
		size: 'xl';
		description: string;
		nodeId: string;
		data: MicrophoneArrayNodeData;
		onCalibrate: (value: MicrophoneArrayNodeData) => Promise<MicrophoneArrayNodeData>;
	};
	let metrics = $state<MicrophoneArrayMetrics | null>(null);
	let unlistenMetrics: (() => void) | undefined;
	$effect(() => {
		if (!audioStore.isRunning) metrics = null;
	});

	let enabledMembers = $derived(data.members.filter((member) => member.enabled && member.quality !== 'excluded').length);
	let configured = $derived(data.sources.length > 0 && enabledMembers >= 2);
	let status = $derived(
		metrics
			? metrics.state === 'ready'
				? `Live · ${metrics.activeAlgorithm === 'delayAndSum' ? 'Delay-and-sum' : metrics.activeAlgorithm.toUpperCase()}`
				: metrics.state === 'fallback'
					? `Fallback · ${metrics.fallbackReason === 'domainUnlocked' ? 'Clock sync' : metrics.fallbackReason === 'noHealthyChannel' ? 'No healthy channel' : 'Safe input'}`
					: metrics.state === 'bypassed'
						? 'Bypassed · best healthy mic'
						: metrics.state === 'error'
							? 'Array source error'
							: 'Synchronizing clocks'
			: !configured
				? 'Setup required'
				: data.calibration.state === 'ready'
					? `Calibrated ${data.calibration.qualityScore ?? 0}%`
					: data.calibration.state === 'needsReview'
						? 'Review calibration'
						: 'Calibration required'
	);
	let algorithmLabel = $derived(
		metrics
			? `${data.algorithm === 'auto' ? 'Auto → ' : ''}${metrics.activeAlgorithm === 'delayAndSum' ? 'Delay-and-sum' : metrics.activeAlgorithm.toUpperCase()}`
			: data.algorithm === 'delayAndSum'
				? 'Delay-and-sum'
				: data.algorithm === 'gsc'
					? 'GSC'
					: data.algorithm === 'mvdr'
						? 'MVDR'
						: 'Auto'
	);

	onMount(async () => {
		unlistenMetrics = await audioMethods.onMicrophoneArrayMetrics((snapshot) => {
			if (snapshot.nodeId === id) metrics = snapshot;
		});
	});

	onDestroy(() => unlistenMetrics?.());

	async function openSetup() {
		const result = await modalManager.open<MicrophoneArrayNodeData, SetupParams>('Microphone Array', Setup, {
			size: 'xl',
			description: 'Combine physical input channels into one spatially focused microphone.',
			nodeId: id,
			data: structuredClone($state.snapshot(data)),
			onCalibrate: audioMethods.calibrateMicrophoneArray
		});
		if (result) flow.updateNodeData(id, result);
	}

	function toggleBypass() {
		flow.updateNodeData(id, { bypassed: !data.bypassed });
	}

	function setStrength(value: number) {
		if (Number.isFinite(value)) flow.updateNodeData(id, { strength: value });
	}

	async function openAttribution() {
		try {
			await openUrl('https://redratinhat.com/products/');
		} catch {
			toast.error('Could not open the Red Rat in Hat website.');
		}
	}
</script>

<Wrapper label="Microphone Array" accent="input" icon={Mic} hasOutput bypassed={data.bypassed} onBypass={toggleBypass}>
	{#snippet badge()}
		{#if data.sources.length > 1}
			<span class="rounded-full border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 font-mono text-[8px] text-amber-800"> EXPERIMENTAL </span>
		{/if}
	{/snippet}

	<div class="relative flex w-56 flex-col gap-3">
		<div class="grid grid-cols-2 gap-2">
			<div class="rounded-lg border border-neutral-300 bg-neutral-100/70 px-2.5 py-2">
				<div class="font-mono text-[9px] text-neutral-700">MICROPHONES</div>
				<div class="mt-0.5 font-mono text-sm text-theme tabular-nums">
					{#if metrics}{metrics.activeChannels}<span class="text-[9px] text-neutral-700">/{enabledMembers}</span>{:else}{enabledMembers}{/if}
				</div>
			</div>
			<div class="rounded-lg border border-neutral-300 bg-neutral-100/70 px-2.5 py-2">
				<div class="font-mono text-[9px] text-neutral-700">CLOCKS</div>
				<div class="mt-0.5 font-mono text-sm text-theme tabular-nums">{data.sources.length}</div>
			</div>
		</div>

		<div class="flex items-center justify-between gap-3">
			<span
				class={[
					'text-[10px]',
					metrics?.state === 'ready' || (!metrics && configured && data.calibration.state === 'ready')
						? 'text-emerald-700'
						: metrics?.state === 'error'
							? 'text-red-700'
							: 'text-amber-800'
				]}>{status}</span>
			<button type="button" class="nodrag nopan button-main primary h-7 rounded-lg px-3 text-[10px] font-semibold" onclick={openSetup}> Setup </button>
		</div>

		<div class="space-y-1.5 border-t border-neutral-300 pt-2.5">
			<div class="flex items-center justify-between text-[9px] text-neutral-800">
				<span>{algorithmLabel}</span><span class="font-mono tabular-nums">{Math.round(data.strength * 100)}%</span>
			</div>
			<input
				class="nodrag nopan nowheel w-full accent-emerald-600"
				type="range"
				aria-label="Array strength"
				min="0"
				max="1"
				step="0.01"
				value={data.strength}
				oninput={(event) => setStrength(event.currentTarget.valueAsNumber)} />
		</div>

		<button
			type="button"
			class="nodrag nopan absolute right-0 -bottom-1.5 font-mono text-[8px] leading-none text-neutral-500 transition-colors hover:text-theme focus-visible:text-theme focus-visible:outline-none"
			aria-label="Microphone Array contribution by Red Rat in Hat"
			title="Microphone Array contribution by Red Rat in Hat"
			onclick={openAttribution}>byRedRatInHat</button>
	</div>
</Wrapper>
