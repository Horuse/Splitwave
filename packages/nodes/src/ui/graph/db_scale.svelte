<script lang="ts" module>
	/** Steps a labelled tick may take, dB; the smallest one that keeps labels apart wins. */
	const STEPS = [1, 2, 3, 5, 6, 10, 12, 20, 30];
	/** Least vertical room between two labels, px. */
	const LABEL_GAP_PX = 13;

	export interface ScaleTick {
		db: number;
		major: boolean;
	}

	/** Labelled ticks every `step` dB from `max` down to `min`, with fine unlabelled ticks between them. */
	export function dbTicks(min: number, max: number, heightPx: number): ScaleTick[] {
		const span = max - min;
		const step = STEPS.find((s) => (heightPx * s) / span >= LABEL_GAP_PX) ?? STEPS[STEPS.length - 1];
		// 1 dB fine ticks while they stay a few px apart, coarser otherwise.
		const fine = heightPx / span >= 3 && step <= 6 ? 1 : step / (step % 5 === 0 ? 5 : 3);
		const ticks: ScaleTick[] = [];
		const count = Math.round(span / fine);
		for (let i = 0; i <= count; i++) {
			const db = Math.round((max - i * fine) * 100) / 100;
			const major = Math.abs((max - db) / step - Math.round((max - db) / step)) < 1e-6;
			ticks.push({ db, major });
		}
		return ticks;
	}
</script>

<script lang="ts">
	interface Props {
		/** Bottom and top of the bar this scale sits beside, dB. */
		min: number;
		max?: number;
		/** Height of that bar, px: the scale fills it exactly. */
		height: number;
		/** Extra labelled values a profile cares about, such as a loudness target. */
		marks?: number[];
	}

	let { min, max = 0, height, marks = [] }: Props = $props();

	let ticks = $derived(dbTicks(min, max, height));

	function pct(db: number): number {
		return ((db - min) / (max - min)) * 100;
	}

	// A mark too close to a labelled tick would overlap its label; the mark wins.
	let shown = $derived.by(() => {
		const minGap = (LABEL_GAP_PX * 0.7 * (max - min)) / height;
		const extra = [...new Set(marks.filter((m) => m > min && m < max))];
		return [
			// A tick at a mark's value is replaced by the mark; a label close to one is dropped.
			...ticks.filter((t) => !extra.includes(t.db) && (!t.major || !extra.some((m) => Math.abs(m - t.db) < minGap))),
			...extra.map((db) => ({ db, major: true }))
		];
	});
</script>

<div class="relative w-7 font-mono text-[8px] text-neutral-600 select-none" style="height: {height}px;">
	{#each shown as t (t.db)}
		<div class="absolute left-0 flex translate-y-1/2 items-center" style="bottom: {pct(t.db)}%; height: 1px;">
			<div class={['h-px shrink-0', t.major ? 'w-1.5 bg-neutral-500' : 'w-1 bg-neutral-300']}></div>
			{#if t.major}<span class="ml-0.5 leading-none">{t.db}</span>{/if}
		</div>
	{/each}
</div>
