<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { LimiterNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import Slider from './_slider.svelte';

	type LimiterNodeType = Node<LimiterNodeData, 'limiter'>;
	let { id, data }: NodeProps<LimiterNodeType> = $props();

	const flow = useSvelteFlow();

	function setThreshold(v: number) {
		const patch = { thresholdDb: v };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}
	function setDrive(v: number) {
		const patch = { driveDb: v };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	// Transfer curve y = c * tanh(x * d / c), mirroring effects.rs LimiterEffect.
	// Plotted over [-1.2, +1.2] so the saturation knee is visible past unity.
	const CURVE_W = 80;
	const CURVE_H = 60;
	const X_MIN = -1.2;
	const X_MAX = 1.2;

	function xPx(x: number): number {
		return ((x - X_MIN) / (X_MAX - X_MIN)) * CURVE_W;
	}
	function yPx(y: number): number {
		return CURVE_H - ((y - X_MIN) / (X_MAX - X_MIN)) * CURVE_H;
	}

	function curvePath(thresholdDb: number, driveDb: number): string {
		const c = Math.pow(10, thresholdDb / 20);
		const d = Math.pow(10, driveDb / 20);
		const steps = 60;
		let path = '';
		for (let i = 0; i <= steps; i++) {
			const x = X_MIN + (X_MAX - X_MIN) * (i / steps);
			const y = c * Math.tanh((x * d) / c);
			path += `${i === 0 ? 'M' : 'L'} ${xPx(x).toFixed(2)} ${yPx(y).toFixed(2)} `;
		}
		return path;
	}

	function ceilingY(): number {
		return yPx(Math.pow(10, data.thresholdDb / 20));
	}
	function ceilingYNeg(): number {
		return yPx(-Math.pow(10, data.thresholdDb / 20));
	}
</script>

<Wrapper label="Limiter" accent="effect" hasInput hasOutput>
	<div class="flex w-50 flex-col gap-1.5">
		<div class="flex items-start gap-2">
			<svg
				viewBox="0 0 {CURVE_W} {CURVE_H}"
				class="h-16 w-20 shrink-0 rounded border border-neutral-300 bg-neutral-100"
				role="img"
				aria-label="Transfer curve preview"
			>
				<line x1={xPx(0)} y1="0" x2={xPx(0)} y2={CURVE_H} stroke="rgb(156 163 175)" stroke-width="0.3" stroke-dasharray="2 2" />
				<line x1="0" y1={yPx(0)} x2={CURVE_W} y2={yPx(0)} stroke="rgb(156 163 175)" stroke-width="0.3" stroke-dasharray="2 2" />
				<line
					x1={xPx(X_MIN)} y1={yPx(X_MIN)}
					x2={xPx(X_MAX)} y2={yPx(X_MAX)}
					stroke="rgb(163 163 163)" stroke-width="0.4" stroke-dasharray="1 2"
				/>
				<line x1="0" y1={ceilingY()} x2={CURVE_W} y2={ceilingY()} stroke="rgb(239 68 68)" stroke-width="0.3" stroke-dasharray="1 1" opacity="0.5" />
				<line x1="0" y1={ceilingYNeg()} x2={CURVE_W} y2={ceilingYNeg()} stroke="rgb(239 68 68)" stroke-width="0.3" stroke-dasharray="1 1" opacity="0.5" />
				<path d={curvePath(data.thresholdDb, data.driveDb)} stroke="rgb(245 158 11)" stroke-width="1.2" fill="none" />
			</svg>
			<div class="flex flex-col gap-0.5 text-[9px] text-neutral-900">
				<span class="rounded bg-neutral-200 px-1 py-0.5 font-mono leading-tight">tanh</span>
				<span class="opacity-60">in → out</span>
				<span class="opacity-60">unity dashed</span>
			</div>
		</div>

		<Slider
			label="Ceiling"
			value={data.thresholdDb}
			min={-24}
			max={0}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[-12, -6, -3, 0]}
			onChange={setThreshold}
		/>
		<Slider
			label="Drive"
			value={data.driveDb}
			min={0}
			max={24}
			step={0.1}
			unit=" dB"
			defaultValue={0}
			ticks={[0, 6, 12, 24]}
			onChange={setDrive}
		/>
	</div>
</Wrapper>
