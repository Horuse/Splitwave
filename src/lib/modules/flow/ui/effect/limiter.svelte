<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { LimiterNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { PresetBar } from '$lib/modules/preset/ui';
	import type { PresetData } from '$lib/modules/preset';
	import Slider from './_slider.svelte';
	import GrBar from './_gr_bar.svelte';

	type LimiterNodeType = Node<LimiterNodeData, 'limiter'>;
	let { id, data }: NodeProps<LimiterNodeType> = $props();

	const flow = useSvelteFlow();

	function setCeiling(v: number) {
		const patch = { ceilingDb: v };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
	function setRelease(v: number) {
		const patch = { releaseMs: v };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
	function setLookahead(v: number) {
		flow.updateNodeData(id, { lookaheadMs: v });
	}

	function applyPreset(p: PresetData<'limiter'>) {
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	// Ceiling transfer curve
	const W = 130, H = 36;
	const DB_MIN = -24, DB_MAX = 6;

	function toSvg(db: number): number {
		return ((db - DB_MIN) / (DB_MAX - DB_MIN)) * W;
	}
	function toSvgY(db: number): number {
		return H - ((db - DB_MIN) / (DB_MAX - DB_MIN)) * H;
	}

	let ceilingSvgX = $derived(toSvg(data.ceilingDb));
</script>

<Wrapper
	label="Limiter"
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex flex-col gap-2">
		<PresetBar kind="limiter" {data} onApply={applyPreset} />

		<div class="flex flex-col gap-1 w-50 nowheel nodrag">
			<svg
				width="100%"
				viewBox="0 0 {W} {H}"
				class="overflow-visible rounded border border-neutral-300 bg-neutral-100 select-none"
			>
				{#each [-18, -12, -6, 0] as db}
					<line x1={toSvg(db)} y1={0} x2={toSvg(db)} y2={H}
						stroke="currentColor" stroke-width="0.5"
						class={db === 0 ? 'text-neutral-400' : 'text-neutral-300'}
					/>
					<text x={toSvg(db)} y={H - 2} font-size="6" fill="currentColor" class="text-neutral-400" text-anchor="middle">{db}</text>
				{/each}
				<rect x={0} y={0} width={ceilingSvgX} height={H} fill="#22c55e" opacity="0.08" />
				<rect x={ceilingSvgX} y={0} width={W - ceilingSvgX} height={H} fill="#ef4444" opacity="0.08" />
				<line
					x1={ceilingSvgX} y1={0}
					x2={ceilingSvgX} y2={H}
					stroke="#ef4444" stroke-width="1.5" stroke-dasharray="3,2"
				/>
				<text x={ceilingSvgX + 2} y={8} font-size="7" fill="#ef4444">{data.ceilingDb.toFixed(1)} dB</text>
			</svg>
			<GrBar nodeId={id} horizontal />
		</div>

		<div class="flex w-50 flex-col gap-1.5">
			<Slider
				label="Ceiling"
				value={data.ceilingDb}
				min={-12}
				max={0}
				step={0.1}
				unit=" dB"
				defaultValue={-0.3}
				ticks={[-6, -3, -1, 0]}
				onChange={setCeiling}
			/>
			<Slider
				label="Lookahead"
				value={data.lookaheadMs}
				min={1}
				max={20}
				step={0.5}
				unit=" ms"
				defaultValue={5}
				ticks={[2, 5, 10]}
				onChange={setLookahead}
			/>
			<Slider
				label="Release"
				value={data.releaseMs}
				min={10}
				max={500}
				step={5}
				unit=" ms"
				defaultValue={50}
				ticks={[50, 100, 250]}
				onChange={setRelease}
			/>
			<p class="text-[9px] text-neutral-500">Lookahead change rebuilds the pipeline.</p>
		</div>
	</div>
</Wrapper>
