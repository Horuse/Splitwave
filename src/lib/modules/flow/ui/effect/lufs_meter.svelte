<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onDestroy, onMount } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { LufsMeterNodeData } from '$lib/modules/pipeline/types';
	import Wrapper from '../node.svelte';

	type LufsMeterNodeType = Node<LufsMeterNodeData, 'lufsMeter'>;
	let { id, data }: NodeProps<LufsMeterNodeType> = $props();

	const flow = useSvelteFlow();

	// Sentinel emitted from the engine when LUFS is `-inf` (silent).
	const LUFS_SILENT = -120;

	const PRESETS: { label: string; subtitle: string; value: number | null }[] = [
		{ label: 'Free', subtitle: '', value: null },
		{ label: '−23', subtitle: 'EBU R128', value: -23 },
		{ label: '−16', subtitle: 'Apple', value: -16 },
		{ label: '−14', subtitle: 'Spotify', value: -14 }
	];

	function setTarget(value: number | null) {
		flow.updateNodeData(id, { target: value });
	}

	let momentary = $state(LUFS_SILENT);
	let shortterm = $state(LUFS_SILENT);
	let integrated = $state(LUFS_SILENT);
	let tpL = $state(LUFS_SILENT);
	let tpR = $state(LUFS_SILENT);

	interface LufsTick {
		nodeId: string;
		momentary: number;
		shortterm: number;
		integrated: number;
		tpL: number;
		tpR: number;
	}

	function format(v: number): string {
		return v <= LUFS_SILENT ? '−∞' : v.toFixed(1);
	}

	function targetClass(integrated: number, target: number | null | undefined): string {
		if (integrated <= LUFS_SILENT) return 'text-neutral-400';
		if (target == null) return 'text-neutral-900';
		const delta = Math.abs(integrated - target);
		if (delta <= 0.5) return 'text-green-600';
		if (delta <= 1.5) return 'text-amber-500';
		return 'text-red-500';
	}

	// Streaming services require −1 dBTP ceiling; clipping is anything ≥ 0 dBTP.
	function tpClass(tp: number): string {
		if (tp <= LUFS_SILENT) return 'text-neutral-400';
		if (tp >= 0) return 'text-red-500';
		if (tp >= -1) return 'text-amber-500';
		return 'text-neutral-900';
	}

	let unlisten: UnlistenFn | undefined;

	onMount(async () => {
		unlisten = await listen<LufsTick>('audio://lufs', (event) => {
			const p = event.payload;
			if (p.nodeId !== id) return;
			momentary = p.momentary;
			shortterm = p.shortterm;
			integrated = p.integrated;
			tpL = p.tpL;
			tpR = p.tpR;
		});
	});

	onDestroy(() => {
		unlisten?.();
	});
</script>

<Wrapper label="LUFS Meter" accent="effect" hasInput hasOutput>
	<div class="flex w-36 flex-col gap-1 font-mono text-[10px]">
		<div class="flex items-baseline justify-between rounded-sm border border-neutral-300 px-1.5 py-0.5">
			<span class="text-neutral-500">M</span>
			<span class="tabular-nums">{format(momentary)}</span>
		</div>
		<div class="flex items-baseline justify-between rounded-sm border border-neutral-300 px-1.5 py-0.5">
			<span class="text-neutral-500">S</span>
			<span class="tabular-nums">{format(shortterm)}</span>
		</div>
		<div class="flex items-baseline justify-between rounded-sm border border-neutral-300 bg-neutral-100 px-1.5 py-0.5 text-[11px]">
			<span class="text-neutral-500">I</span>
			<span class="tabular-nums font-semibold {targetClass(integrated, data.target)}">{format(integrated)}</span>
		</div>
		<div class="text-center text-[8px] text-neutral-500">LUFS</div>

		<div class="nodrag nopan mt-0.5 grid grid-cols-4 gap-[2px] rounded-sm border border-neutral-300 p-[2px]">
			{#each PRESETS as preset (preset.label)}
				<button
					type="button"
					onclick={() => setTarget(preset.value)}
					title={preset.subtitle || 'No target'}
					class={[
						'flex flex-col items-center rounded-sm py-0.5 leading-none transition-colors',
						data.target === preset.value
							? 'bg-neutral-900 text-white'
							: 'bg-neutral-100 text-neutral-900 hover:bg-neutral-200'
					]}
				>
					<span class="font-mono text-[9px] tabular-nums">{preset.label}</span>
					{#if preset.subtitle}
						<span class="text-[7px] opacity-70">{preset.subtitle}</span>
					{/if}
				</button>
			{/each}
		</div>

		<div class="mt-0.5 flex items-baseline justify-between rounded-sm border border-neutral-300 px-1.5 py-0.5">
			<span class="text-neutral-500">TP L</span>
			<span class="tabular-nums font-semibold {tpClass(tpL)}">{format(tpL)}</span>
		</div>
		<div class="flex items-baseline justify-between rounded-sm border border-neutral-300 px-1.5 py-0.5">
			<span class="text-neutral-500">TP R</span>
			<span class="tabular-nums font-semibold {tpClass(tpR)}">{format(tpR)}</span>
		</div>
		<div class="text-center text-[8px] text-neutral-500">dBTP</div>
	</div>
</Wrapper>
