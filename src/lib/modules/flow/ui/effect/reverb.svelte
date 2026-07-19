<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { ReverbNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { Reverb } from '$lib/components/icons';
	import { PresetBar } from '$lib/modules/preset/ui';
	import type { PresetData } from '$lib/modules/preset';
	import Slider from './_slider.svelte';

	type ReverbNodeType = Node<ReverbNodeData, 'reverb'>;
	let { id, data }: NodeProps<ReverbNodeType> = $props();

	const flow = useSvelteFlow();

	function patch<K extends keyof ReverbNodeData>(key: K, value: ReverbNodeData[K]) {
		const p = { [key]: value } as Partial<ReverbNodeData>;
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function pctFmt(v: number): string {
		return `${Math.round(v * 100)}%`;
	}

	function applyPreset(p: PresetData<'reverb'>) {
		flow.updateNodeData(id, p);
		audioMethods.updateEffect(id, p).catch(() => {});
	}

	function toggleBypass() {
		patch('bypassed', !data.bypassed);
	}

	// Decay envelope visualization
	const W = 208, H = 44;

	// RT60 approximation: roomSize 0-1 maps to ~0.1-3s; more damping = faster decay
	let envelopePath = $derived(() => {
		const pts: string[] = [];
		const N = 120;
		const rt60 = 0.1 + data.roomSize * 2.9; // seconds
		const decayRate = (6 / rt60) * (1 + data.damping * 1.5);
		// initial attack transient
		pts.push(`0,${H}`);
		pts.push(`3,0`);
		for (let i = 1; i <= N; i++) {
			const t = (i / N) * 3;
			const amp = Math.exp(-decayRate * t);
			const y = H - amp * H;
			const x = 3 + (i / N) * (W - 3);
			pts.push(`${x.toFixed(1)},${y.toFixed(1)}`);
		}
		return pts.join(' ');
	});

	// stereo width offset band
	let widthBandPath = $derived(() => {
		const N = 60;
		const rt60 = 0.1 + data.roomSize * 2.9;
		const decayRate = (6 / rt60) * (1 + data.damping * 1.5);
		const spread = data.width * 0.18;
		const upper: string[] = [];
		const lower: string[] = [];
		for (let i = 0; i <= N; i++) {
			const t = (i / N) * 3;
			const amp = Math.exp(-decayRate * t);
			const x = 3 + (i / N) * (W - 3);
			const cy = H - amp * H;
			upper.push(`${x.toFixed(1)},${Math.max(0, cy - amp * H * spread).toFixed(1)}`);
			lower.push(`${x.toFixed(1)},${Math.min(H, cy + amp * H * spread).toFixed(1)}`);
		}
		return upper.join(' ') + ' ' + lower.reverse().join(' ');
	});
</script>

<Wrapper
	label="Reverb"
	icon={Reverb}
	accent="effect"
	hasInput
	hasOutput
	channelIo
	nodeId={id}
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex flex-col gap-2">
		<PresetBar kind="reverb" {data} onApply={applyPreset} />

		<div class="nowheel nodrag">
			<svg
				width={W}
				height={H}
				class="overflow-visible rounded border border-neutral-300 bg-neutral-100 select-none"
			>
				<polygon
					points={widthBandPath()}
					fill="#3b82f6" opacity="0.12"
				/>
				<polyline
					points={envelopePath()}
					fill="none" stroke="#3b82f6" stroke-width="1.5" stroke-linejoin="round"
				/>
				<text x={W - 2} y={H - 2} font-size="7" fill="currentColor" class="text-neutral-400" text-anchor="end">
					wet {Math.round(data.mix * 100)}%
				</text>
			</svg>
		</div>

		<div class="flex w-52 flex-col gap-1.5">
			<Slider
				label="Room"
				value={data.roomSize}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={0.5}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('roomSize', v)}
			/>
			<Slider
				label="Damping"
				value={data.damping}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={0.5}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('damping', v)}
			/>
			<Slider
				label="Width"
				value={data.width}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={1}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('width', v)}
			/>
			<Slider
				label="Mix"
				value={data.mix}
				min={0}
				max={1}
				step={0.01}
				format={pctFmt}
				defaultValue={0.33}
				ticks={[0.25, 0.5, 0.75]}
				onChange={(v) => patch('mix', v)}
			/>
		</div>
	</div>
</Wrapper>
