<script lang="ts">
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { EqNodeData } from '$lib/modules/pipeline/types';
	import { methods as audioMethods } from '$lib/modules/audio/methods';
	import Wrapper from '../node.svelte';
	import { Combobox } from '$lib/modules/form/ui';

	type EqNodeType = Node<EqNodeData, 'eq'>;
	let { id, data }: NodeProps<EqNodeType> = $props();

	const flow = useSvelteFlow();

	// Mirrors EQ_FREQUENCIES_HZ / EQ_CROSSOVER_FREQS in audio/effects.rs.
	const FREQUENCIES = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000] as const;
	const CROSSOVERS = [
		45.2548, 89.4427, 176.7767, 353.5534, 707.1068, 1414.2136, 2828.4271, 5656.8542, 11313.7085
	] as const;
	const GAIN_MIN = -18;
	const GAIN_MAX = 18;

	const F_MIN = 20;
	const F_MAX = 20_000;
	const DB_RANGE = 18;
	const CURVE_W = 280;
	const CURVE_H = 70;
	const POINTS = 200;

	const PRESETS: Record<string, number[]> = {
		Flat: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
		'Bass Boost': [6, 5, 4, 2, 0, 0, 0, 0, 0, 0],
		'Treble Boost': [0, 0, 0, 0, 0, 0, 2, 4, 5, 6],
		Vocal: [-4, -3, -1, 1, 3, 4, 4, 2, 0, -2],
		Podcast: [-6, -4, -2, 0, 2, 3, 4, 3, 1, -1],
		Rock: [4, 3, 1, -1, -1, 1, 3, 4, 5, 5],
		Pop: [-1, 0, 1, 2, 3, 2, 0, -1, -1, -2],
		Jazz: [3, 2, 1, 1, -1, -1, 0, 1, 2, 3],
		Classical: [4, 3, 2, 0, 0, 0, -1, -1, -2, -3],
		Electronic: [4, 3, 1, 0, -2, 1, 0, 1, 3, 4]
	};

	const presetOptions = Object.keys(PRESETS).map((name) => ({ value: name, label: name }));

	function patchGains(gains: number[]) {
		const patch = { gainsDb: gains };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch as Record<string, unknown>).catch(() => {});
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		audioMethods.updateEffect(id, patch).catch(() => {});
	}

	function setBand(i: number, db: number) {
		const next = data.gainsDb.slice();
		next[i] = Math.max(GAIN_MIN, Math.min(GAIN_MAX, db));
		patchGains(next);
	}

	function applyPreset(name: string | null) {
		if (!name || !(name in PRESETS)) return;
		patchGains(PRESETS[name].slice());
	}

	function matchingPresetName(gains: number[]): string | null {
		for (const [name, p] of Object.entries(PRESETS)) {
			if (gains.every((g, i) => Math.abs(g - p[i]) < 0.05)) return name;
		}
		return null;
	}

	let selectedPreset = $derived(matchingPresetName(data.gainsDb));

	// LR4 magnitudes — matches the Rust splitter so the curve shows true output.
	function lr4LpfMag(f: number, fc: number): number {
		const r = f / fc;
		const r4 = r * r * r * r;
		return 1 / (1 + r4);
	}
	function lr4HpfMag(f: number, fc: number): number {
		const r = f / fc;
		const r4 = r * r * r * r;
		return r4 / (1 + r4);
	}
	function bandMag(i: number, f: number): number {
		let mag = 1;
		for (let j = 0; j < i; j++) mag *= lr4HpfMag(f, CROSSOVERS[j]);
		if (i < 9) mag *= lr4LpfMag(f, CROSSOVERS[i]);
		return mag;
	}
	function combinedDbAt(freq: number): number {
		let total = 0;
		for (let i = 0; i < 10; i++) {
			const gainLinear = Math.pow(10, (data.gainsDb[i] ?? 0) / 20);
			total += gainLinear * bandMag(i, freq);
		}
		return 20 * Math.log10(Math.max(total, 1e-12));
	}

	let curvePath = $derived.by(() => {
		const lnMin = Math.log(F_MIN);
		const lnMax = Math.log(F_MAX);
		let d = '';
		for (let i = 0; i < POINTS; i++) {
			const t = i / (POINTS - 1);
			const f = Math.exp(lnMin + t * (lnMax - lnMin));
			const x = t * CURVE_W;
			const y = dbToY(combinedDbAt(f));
			d += `${i === 0 ? 'M' : 'L'} ${x.toFixed(2)} ${y.toFixed(2)} `;
		}
		return d;
	});

	function freqToX(hz: number): number {
		return ((Math.log(hz) - Math.log(F_MIN)) / (Math.log(F_MAX) - Math.log(F_MIN))) * CURVE_W;
	}
	function dbToY(db: number): number {
		return CURVE_H / 2 - (Math.max(-DB_RANGE, Math.min(DB_RANGE, db)) / DB_RANGE) * (CURVE_H / 2);
	}
	const dbGrid = [12, 6, 0, -6, -12];

	// --- Vertical fader drag (replaces <input type=range> for a mixer feel). ---
	const FADER_H = 96;
	let dragging = $state<{ band: number; pointerId: number } | null>(null);
	let faderEls = $state<(HTMLDivElement | undefined)[]>(new Array(10));

	function gainToFaderPct(g: number): number {
		return ((GAIN_MAX - g) / (GAIN_MAX - GAIN_MIN)) * 100;
	}

	function clientYToGain(yClient: number, rect: DOMRect): number {
		const pct = (yClient - rect.top) / rect.height;
		return GAIN_MAX - Math.max(0, Math.min(1, pct)) * (GAIN_MAX - GAIN_MIN);
	}

	function onFaderPointerDown(i: number, e: PointerEvent) {
		const el = faderEls[i];
		if (!el) return;
		el.setPointerCapture(e.pointerId);
		dragging = { band: i, pointerId: e.pointerId };
		setBand(i, clientYToGain(e.clientY, el.getBoundingClientRect()));
	}
	function onFaderPointerMove(i: number, e: PointerEvent) {
		if (!dragging || dragging.band !== i) return;
		const el = faderEls[i];
		if (!el) return;
		setBand(i, clientYToGain(e.clientY, el.getBoundingClientRect()));
	}
	function onFaderPointerUp(i: number, e: PointerEvent) {
		const el = faderEls[i];
		if (el && el.hasPointerCapture(e.pointerId)) el.releasePointerCapture(e.pointerId);
		dragging = null;
	}
	function onFaderDoubleClick(i: number) {
		setBand(i, 0);
	}
	function onFaderWheel(i: number, e: WheelEvent) {
		e.preventDefault();
		const step = e.shiftKey ? 0.1 : 1;
		setBand(i, (data.gainsDb[i] ?? 0) + (e.deltaY > 0 ? -step : step));
	}

	function formatFreq(hz: number): string {
		if (hz >= 1000) return `${hz / 1000}k`;
		return String(hz);
	}
	function formatGain(g: number): string {
		const v = g.toFixed(1);
		return g > 0 ? `+${v}` : v;
	}
</script>

<Wrapper
	label="EQ"
	accent="effect"
	hasInput
	hasOutput
	bypassed={data.bypassed}
	onBypass={toggleBypass}
>
	<div class="flex w-72 flex-col gap-1.5">
		<Combobox
			options={presetOptions}
			value={selectedPreset}
			placeholder="Custom"
			onChange={applyPreset}
		/>

		<svg
			viewBox="0 0 {CURVE_W} {CURVE_H}"
			class="h-16 w-full rounded border border-neutral-300 bg-neutral-100"
			role="img"
			aria-label="EQ frequency response"
		>
			{#each dbGrid as db (db)}
				<line
					x1="0" y1={dbToY(db)} x2={CURVE_W} y2={dbToY(db)}
					stroke="rgb(156 163 175)" stroke-width="0.3"
					stroke-dasharray={db === 0 ? '' : '2 2'}
					opacity={db === 0 ? 0.6 : 0.3}
				/>
			{/each}
			<path d={curvePath} stroke="rgb(245 158 11)" stroke-width="1.5" fill="none" />
			{#each FREQUENCIES as freq (freq)}
				<circle
					cx={freqToX(freq)}
					cy={dbToY(combinedDbAt(freq))}
					r="2"
					fill="rgb(245 158 11)"
				/>
			{/each}
		</svg>

		<div class="flex gap-0.5">
			{#each FREQUENCIES as freq, i (freq)}
				{@const gain = data.gainsDb[i] ?? 0}
				<div class="flex flex-1 flex-col items-center gap-0.5">
					<span class="font-mono text-[8px] text-neutral-1000 tabular-nums">{formatGain(gain)}</span>
					<div
						bind:this={faderEls[i]}
						class="nodrag nopan nowheel relative h-24 w-full cursor-ns-resize rounded-sm border border-neutral-400 bg-neutral-200/60"
						style="touch-action: none;"
						onpointerdown={(e) => onFaderPointerDown(i, e)}
						onpointermove={(e) => onFaderPointerMove(i, e)}
						onpointerup={(e) => onFaderPointerUp(i, e)}
						onpointercancel={(e) => onFaderPointerUp(i, e)}
						ondblclick={() => onFaderDoubleClick(i)}
						onwheel={(e) => onFaderWheel(i, e)}
						role="slider"
						tabindex="0"
						aria-label="{freq} Hz"
						aria-valuemin={GAIN_MIN}
						aria-valuemax={GAIN_MAX}
						aria-valuenow={gain}
					>
						<!-- Vertical rail through centre + tick marks at ±6 / ±12 dB. -->
						<div class="pointer-events-none absolute top-1 bottom-1 left-1/2 w-px -translate-x-1/2 bg-neutral-500/60"></div>
						{#each [12, 6, -6, -12] as tick (tick)}
							<div
								class="pointer-events-none absolute left-1/4 -translate-y-1/2 h-px w-1/2 bg-neutral-500/30"
								style="top: {gainToFaderPct(tick)}%;"
							></div>
						{/each}
						<div class="pointer-events-none absolute top-1/2 right-0.5 left-0.5 h-px -translate-y-1/2 bg-neutral-700"></div>
						<div
							class="pointer-events-none absolute right-0 left-0 h-2 -translate-y-1/2 rounded-sm bg-amber-500 shadow"
							style="top: {gainToFaderPct(gain)}%;"
						></div>
					</div>
					<span class="font-mono text-[8px] text-neutral-900">{formatFreq(freq)}</span>
				</div>
			{/each}
		</div>
	</div>
</Wrapper>
