<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { CompressorNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';
	import GrBar from './_gr_bar.svelte';
	import { PresetBar } from '$lib/modules/preset/ui';
	import type { PresetData } from '$lib/modules/preset';

	type CompressorNodeType = Node<CompressorNodeData, 'compressor'>;
	let { id, data }: NodeProps<CompressorNodeType> = $props();

	const flow = useSvelteFlow();

	function patch<K extends keyof CompressorNodeData>(key: K, value: CompressorNodeData[K]) {
		const p = { [key]: value } as Partial<CompressorNodeData>;
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function applyPreset(p: PresetData<'compressor'>) {
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function ratioFmt(v: number): string {
		return `${v.toFixed(1)}:1`;
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}

	// Transfer curve math
	const W = 130, H = 60;
	const X_MIN = -60, X_MAX = 0;
	const Y_MIN = -60, Y_MAX = 12;

	function xToSvg(db: number): number {
		return ((db - X_MIN) / (X_MAX - X_MIN)) * W;
	}
	function yToSvg(db: number): number {
		return H - ((db - Y_MIN) / (Y_MAX - Y_MIN)) * H;
	}

	function transferOut(xDb: number): number {
		const halfKnee = data.kneeDb / 2;
		const over = xDb - data.thresholdDb;
		let gr = 0;
		if (data.kneeDb > 0 && over > -halfKnee && over < halfKnee) {
			const x = over + halfKnee;
			gr = (1 - 1 / data.ratio) * x * x / (2 * data.kneeDb);
		} else if (over > 0) {
			gr = over * (1 - 1 / data.ratio);
		}
		return xDb - gr + data.makeupDb;
	}

	const N = 80;
	let curvePath = $derived(
		Array.from({ length: N + 1 }, (_, i) => {
			const xDb = X_MIN + (i / N) * (X_MAX - X_MIN);
			const yDb = Math.max(Y_MIN, Math.min(Y_MAX, transferOut(xDb)));
			return `${xToSvg(xDb).toFixed(1)},${yToSvg(yDb).toFixed(1)}`;
		}).join(' ')
	);

	// Unity line endpoints
	const unityStart = `${xToSvg(X_MIN)},${yToSvg(X_MIN + 0)}`;
	const unityEnd = `${xToSvg(X_MAX)},${yToSvg(X_MAX)}`;

	// Grid lines
	const xGridLines = [-40, -20];
	const yGridLines = [-40, -20, 0];
</script>

<Wrapper
	label="Compressor"
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
		<PresetBar kind="compressor" {data} onApply={applyPreset} />

		<div class="flex flex-col gap-1 w-52 nowheel nodrag">
			<svg
				width="100%"
				viewBox="0 0 {W} {H}"
				class="overflow-visible rounded border border-neutral-300 bg-neutral-100 select-none"
			>
				{#each xGridLines as x}
					<line
						x1={xToSvg(x)} y1={0}
						x2={xToSvg(x)} y2={H}
						stroke="currentColor" stroke-width="0.5" class="text-neutral-300"
					/>
				{/each}
				{#each yGridLines as y}
					<line
						x1={0} y1={yToSvg(y)}
						x2={W} y2={yToSvg(y)}
						stroke="currentColor" stroke-width="0.5"
						class={y === 0 ? 'text-neutral-400' : 'text-neutral-300'}
					/>
				{/each}
				<line
					x1={xToSvg(data.thresholdDb)} y1={0}
					x2={xToSvg(data.thresholdDb)} y2={H}
					stroke="#f59e0b" stroke-width="0.8" stroke-dasharray="2,2" opacity="0.7"
				/>
				<polyline
					points="{unityStart} {unityEnd}"
					fill="none" stroke="currentColor" stroke-width="0.7"
					class="text-neutral-400" stroke-dasharray="3,2"
				/>
				<polyline
					points={curvePath}
					fill="none" stroke="#3b82f6" stroke-width="1.5" stroke-linejoin="round"
				/>
				<text x={xToSvg(-30)} y={H - 2} font-size="7" fill="currentColor" class="text-neutral-400" text-anchor="middle">in dB</text>
			</svg>
			<GrBar nodeId={id} horizontal />
		</div>

		<div class="flex w-52 flex-col gap-1.5">
			<Slider
				label="Threshold"
				value={data.thresholdDb}
				min={-60}
				max={0}
				step={0.5}
				unit=" dB"
				defaultValue={-18}
				ticks={[-40, -20, -10]}
				onChange={(v) => patch('thresholdDb', v)}
			/>
			<Slider
				label="Ratio"
				value={data.ratio}
				min={1}
				max={20}
				step={0.1}
				format={ratioFmt}
				defaultValue={3}
				ticks={[2, 4, 8]}
				onChange={(v) => patch('ratio', v)}
			/>
			<Slider
				label="Attack"
				value={data.attackMs}
				min={0.1}
				max={100}
				step={0.1}
				unit=" ms"
				defaultValue={10}
				ticks={[1, 10, 50]}
				onChange={(v) => patch('attackMs', v)}
			/>
			<Slider
				label="Release"
				value={data.releaseMs}
				min={10}
				max={1000}
				step={5}
				unit=" ms"
				defaultValue={100}
				ticks={[50, 250, 500]}
				onChange={(v) => patch('releaseMs', v)}
			/>
			<Slider
				label="Knee"
				value={data.kneeDb}
				min={0}
				max={24}
				step={0.5}
				unit=" dB"
				defaultValue={6}
				ticks={[0, 6, 12]}
				onChange={(v) => patch('kneeDb', v)}
			/>
			<Slider
				label="Makeup"
				value={data.makeupDb}
				min={0}
				max={24}
				step={0.1}
				unit=" dB"
				defaultValue={0}
				ticks={[0, 6, 12]}
				onChange={(v) => patch('makeupDb', v)}
			/>
		</div>
	</div>
</Wrapper>
