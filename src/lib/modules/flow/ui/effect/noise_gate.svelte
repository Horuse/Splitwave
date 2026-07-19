<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onMount, onDestroy } from 'svelte';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { NoiseGateNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { PresetBar } from '$lib/modules/preset/ui';
	import type { PresetData } from '$lib/modules/preset';
	import Slider from './_slider.svelte';

	type NoiseGateNodeType = Node<NoiseGateNodeData, 'noiseGate'>;
	let { id, data }: NodeProps<NoiseGateNodeType> = $props();

	const flow = useSvelteFlow();

	function patch<K extends keyof NoiseGateNodeData>(key: K, value: NoiseGateNodeData[K]) {
		const p = { [key]: value } as Partial<NoiseGateNodeData>;
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function applyPreset(p: PresetData<'noiseGate'>) {
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}

	let stateGain = $state(1);
	let gateState = $derived(
		stateGain > 0.85 ? 'open' : stateGain > 0.15 ? 'hold' : 'closed'
	);

	let unlisten: UnlistenFn | undefined;
	onMount(async () => {
		unlisten = await listen<{ nodeId: string; grLin: number }>('audio://gr', (event) => {
			if (event.payload.nodeId !== id) return;
			stateGain = Math.max(0, Math.min(1, event.payload.grLin));
		});
	});
	onDestroy(() => unlisten?.());

	const W = 150, H = 70;
	const X_MIN = -80, X_MAX = 0;
	const Y_MIN = -80, Y_MAX = 6;

	function xToSvg(db: number): number {
		return ((db - X_MIN) / (X_MAX - X_MIN)) * W;
	}
	function yToSvg(db: number): number {
		return H - ((db - Y_MIN) / (Y_MAX - Y_MIN)) * H;
	}

	let gatePath = $derived(() => {
		const thr = data.thresholdDb;
		const range = data.rangeDb;
		const pts: string[] = [];
		for (let i = 0; i <= 100; i++) {
			const xDb = X_MIN + (i / 100) * (X_MAX - X_MIN);
			let yDb: number;
			if (xDb >= thr) {
				yDb = xDb;
			} else {
				// linear transition zone around threshold (±3 dB)
				const zone = 3;
				const t = Math.max(0, Math.min(1, (xDb - (thr - zone)) / zone));
				const rangeOffset = range - xDb;
				yDb = xDb + rangeOffset * (1 - t * t);
			}
			pts.push(`${xToSvg(xDb).toFixed(1)},${yToSvg(Math.max(Y_MIN, Math.min(Y_MAX, yDb))).toFixed(1)}`);
		}
		return pts.join(' ');
	});
</script>

<Wrapper
	label="Noise Gate"
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	inputs={[{ id: 'sidechain', label: 'Sidechain', position: 'bottom' }]}
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex flex-col gap-2">
		<PresetBar kind="noiseGate" {data} onApply={applyPreset} />

		<div class="flex gap-3 items-end nowheel nodrag">
			<svg
				width={W}
				height={H}
				class="overflow-visible rounded border border-neutral-300 bg-neutral-100 select-none"
			>
				{#each [-60, -40, -20] as x}
					<line x1={xToSvg(x)} y1={0} x2={xToSvg(x)} y2={H}
						stroke="currentColor" stroke-width="0.5" class="text-neutral-300" />
				{/each}
				{#each [-60, -40, -20, 0] as y}
					<line x1={0} y1={yToSvg(y)} x2={W} y2={yToSvg(y)}
						stroke="currentColor" stroke-width="0.5"
						class={y === 0 ? 'text-neutral-400' : 'text-neutral-300'} />
				{/each}
				<line
					x1={xToSvg(data.thresholdDb)} y1={0}
					x2={xToSvg(data.thresholdDb)} y2={H}
					stroke="#f59e0b" stroke-width="0.8" stroke-dasharray="2,2" opacity="0.7"
				/>
				<line
					x1={xToSvg(X_MIN)} y1={yToSvg(X_MIN)}
					x2={xToSvg(X_MAX)} y2={yToSvg(X_MAX)}
					stroke="currentColor" stroke-width="0.7"
					class="text-neutral-400" stroke-dasharray="3,2"
				/>
				<polyline
					points={gatePath()}
					fill="none" stroke="#3b82f6" stroke-width="1.5" stroke-linejoin="round"
				/>
			</svg>

			<div class="flex flex-col gap-1.5 items-center my-auto select-none">
				{#each [
					{ label: 'Open', state: 'open', color: 'bg-green-500' },
					{ label: 'Hold', state: 'hold', color: 'bg-amber-400' },
					{ label: 'Closed', state: 'closed', color: 'bg-neutral-400' }
				] as item}
					<div class="flex items-center gap-1.5">
						<div class="size-2 rounded-full border border-neutral-300 {gateState === item.state ? item.color : 'bg-neutral-200'}
							{gateState === item.state && item.state === 'open' ? 'shadow-[0_0_4px_#22c55e]' : ''}
							{gateState === item.state && item.state === 'hold' ? 'shadow-[0_0_4px_#fbbf24]' : ''}
						"></div>
						<span class="text-[8px] text-neutral-500">{item.label}</span>
					</div>
				{/each}
			</div>
		</div>

		<div class="flex w-52 flex-col gap-1.5">
			<Slider
				label="Threshold"
				value={data.thresholdDb}
				min={-80}
				max={0}
				step={0.5}
				unit=" dB"
				defaultValue={-40}
				ticks={[-60, -40, -20]}
				onChange={(v) => patch('thresholdDb', v)}
			/>
			<Slider
				label="Range"
				value={data.rangeDb}
				min={-80}
				max={0}
				step={0.5}
				unit=" dB"
				defaultValue={-40}
				ticks={[-60, -40, -20]}
				onChange={(v) => patch('rangeDb', v)}
			/>
			<Slider
				label="Attack"
				value={data.attackMs}
				min={0.1}
				max={50}
				step={0.1}
				unit=" ms"
				defaultValue={1}
				ticks={[1, 5, 20]}
				onChange={(v) => patch('attackMs', v)}
			/>
			<Slider
				label="Hold"
				value={data.holdMs}
				min={0}
				max={500}
				step={5}
				unit=" ms"
				defaultValue={50}
				ticks={[20, 100, 250]}
				onChange={(v) => patch('holdMs', v)}
			/>
			<Slider
				label="Release"
				value={data.releaseMs}
				min={10}
				max={1000}
				step={5}
				unit=" ms"
				defaultValue={200}
				ticks={[100, 300, 500]}
				onChange={(v) => patch('releaseMs', v)}
			/>
		</div>
	</div>
</Wrapper>
