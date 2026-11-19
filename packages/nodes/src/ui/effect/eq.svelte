<script lang="ts">
	import { getEnginePort } from '../../engine';
	import { EQ_CROSSOVER_FREQS } from '@splitwave/nodes/generated/engine';
	import { useSvelteFlow, type Node, type NodeProps } from '@xyflow/svelte';
	import type { EqNodeData } from '@splitwave/nodes';
	import Wrapper from '../node.svelte';
	import Sliders from '~icons/fluent/options-16-filled';
	import PresetBar from '../preset_bar.svelte';
	import type { PresetData } from '../../presets';
	import { formatFreq, formatGain } from '@splitwave/nodes';

	type EqNodeType = Node<EqNodeData, 'eq'>;
	let { id, data }: NodeProps<EqNodeType> = $props();

	const engine = getEnginePort();

	const flow = useSvelteFlow();

	// ISO octave band centres; the engine splits them at EQ_CROSSOVER_FREQS.
	const FREQUENCIES = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000] as const;
	const CROSSOVERS = EQ_CROSSOVER_FREQS;
	const GAIN_MIN = -18;
	const GAIN_MAX = 18;

	const F_MIN = 20;
	const F_MAX = 20_000;
	const DB_RANGE = 18;
	const CURVE_W = 325;
	const CURVE_H = 70;
	const POINTS = 200;

	function patchGains(gains: number[]) {
		const patch = { gainsDb: gains };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch as Record<string, unknown>).catch(() => {});
	}

	function toggleBypass() {
		const patch = { bypassed: !data.bypassed };
		flow.updateNodeData(id, patch);
		engine.updateEffect(id, patch).catch(() => {});
	}

	function setBand(i: number, db: number) {
		const next = data.gainsDb.slice();
		next[i] = Math.max(GAIN_MIN, Math.min(GAIN_MAX, db));
		patchGains(next);
	}

	function applyPreset(p: PresetData<'eq'>) {
		patchGains(p.gainsDb.slice());
	}

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
		const g = GAIN_MAX - Math.max(0, Math.min(1, pct)) * (GAIN_MAX - GAIN_MIN);
		return Math.round(g * 10) / 10;
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

	let editingBand = $state<number | null>(null);
	let draft = $state('');

	function startEdit(i: number, e: FocusEvent) {
		editingBand = i;
		draft = String(data.gainsDb[i] ?? 0);
		(e.currentTarget as HTMLInputElement).select();
	}
	function commitEdit(i: number) {
		editingBand = null;
		const next = Number(draft);
		if (!Number.isNaN(next)) setBand(i, next);
	}
	function onEditKeyDown(e: KeyboardEvent) {
		if (e.key === 'Enter') {
			e.preventDefault();
			(e.currentTarget as HTMLInputElement).blur();
		} else if (e.key === 'Escape') {
			e.preventDefault();
			editingBand = null;
			(e.currentTarget as HTMLInputElement).blur();
		}
	}
</script>

<Wrapper label="EQ" icon={Sliders} accent="effect" hasInput hasOutput channelIo nodeId={id} bypassed={data.bypassed} onBypass={toggleBypass}>
	<div class="flex w-72 flex-col gap-1.5">
		<PresetBar kind="eq" {data} onApply={applyPreset} />

		<div class="overflow-hidden rounded-md border border-theme/5 bg-neutral-100">
			<svg viewBox="0 0 {CURVE_W} {CURVE_H}" class="block h-20 w-full" role="img" aria-label="EQ frequency response">
				{#each dbGrid as db (db)}
					<line x1="0" y1={dbToY(db)} x2={CURVE_W} y2={dbToY(db)} class={db === 0 ? 'stroke-neutral-500' : 'stroke-neutral-300'} stroke-width="0.5" />
					{#if db !== 0}
						<text x="3" y={dbToY(db) - 1.5} class="fill-neutral-500 font-mono" font-size="6">{db > 0 ? '+' : ''}{db}</text>
					{/if}
				{/each}
				{#each [100, 1000, 10000] as f (f)}
					<line x1={freqToX(f)} y1="0" x2={freqToX(f)} y2={CURVE_H} class="stroke-neutral-300" stroke-width="0.5" />
					<text x={freqToX(f) + 2} y={CURVE_H - 2} class="fill-neutral-500 font-mono" font-size="6">{formatFreq(f)}</text>
				{/each}
				<path d="{curvePath} L {CURVE_W} {dbToY(0)} L 0 {dbToY(0)} Z" class="fill-signal/15" />
				<path d={curvePath} class="stroke-signal" stroke-width="1.5" fill="none" />
				{#each FREQUENCIES as freq, i (freq)}
					{@const g = data.gainsDb[i] ?? 0}
					<circle
						cx={freqToX(freq)}
						cy={dbToY(combinedDbAt(freq))}
						r="2.2"
						class={g > 0 ? 'fill-signal' : g < 0 ? 'fill-sky-500' : 'fill-neutral-600'} />
				{/each}
			</svg>
		</div>

		<div class="flex gap-0.5">
			{#each FREQUENCIES as freq, i (freq)}
				{@const gain = data.gainsDb[i] ?? 0}
				{@const capTop = gainToFaderPct(gain)}
				<div class="flex flex-1 flex-col items-center gap-2">
					<input
						type="text"
						inputmode="decimal"
						class="nodrag nopan w-full rounded-sm bg-transparent text-center font-mono text-[8px] text-neutral-1000 tabular-nums focus:bg-(--lv-1) focus:ring-1 focus:ring-(--lv-4) focus:outline-none"
						value={editingBand === i ? draft : formatGain(gain)}
						oninput={(e) => (draft = e.currentTarget.value)}
						onfocus={(e) => startEdit(i, e)}
						onblur={() => commitEdit(i)}
						onkeydown={onEditKeyDown} />
					<div
						bind:this={faderEls[i]}
						class="nodrag nopan nowheel relative h-36 w-4 cursor-ns-resize rounded-md border border-theme/5 bg-neutral-100"
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
						aria-valuenow={gain}>
						<!-- Recessed travel slot the cap rides in. -->
						<div class="pointer-events-none absolute inset-y-1 left-1/2 w-1 -translate-x-1/2 rounded-full bg-neutral-300"></div>
						{#each [12, 6, -6, -12] as tick (tick)}
							<div
								class="pointer-events-none absolute left-1 h-px w-1.5 -translate-y-1/2 bg-neutral-500/40"
								style="top: {gainToFaderPct(tick)}%;">
							</div>
							<div
								class="pointer-events-none absolute right-1 h-px w-1.5 -translate-y-1/2 bg-neutral-500/40"
								style="top: {gainToFaderPct(tick)}%;">
							</div>
						{/each}
						<!-- 0 dB detent. -->
						<div class="pointer-events-none absolute top-1/2 right-1 left-1 h-px -translate-y-1/2 bg-neutral-600/70"></div>
						<!-- Boost/cut fill from the 0 dB detent to the cap. -->
						<div
							class="pointer-events-none absolute left-1/2 w-1 -translate-x-1/2 rounded-full {gain >= 0 ? 'bg-signal' : 'bg-sky-500'}"
							style={gain >= 0 ? `top: ${capTop}%; bottom: 50%;` : `top: 50%; bottom: ${100 - capTop}%;`}>
						</div>
						<!-- Physical fader cap: slightly wider than the track, with a grip line. -->
						<div
							class="pointer-events-none absolute left-1/2 h-2 w-[150%] -translate-x-1/2 -translate-y-1/2 rounded-full border border-neutral-400 bg-white"
							style="top: {capTop}%;">
						</div>
					</div>
					<span class="font-mono text-[8px] text-neutral-900">{formatFreq(freq)}</span>
				</div>
			{/each}
		</div>
	</div>
</Wrapper>
