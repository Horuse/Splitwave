<script lang="ts">
	import { useGainReduction } from '../state/gain_reduction.svelte';

	let { nodeId, horizontal = false }: { nodeId: string; horizontal?: boolean } = $props();

	const gr = useGainReduction(() => nodeId);

	const GR_MAX = 20;
	const ticks = [3, 6, 12];

	function pct(db: number): number {
		return Math.min(100, Math.max(0, (db / GR_MAX) * 100));
	}

	function fill(db: number): string {
		if (db >= 6) return 'bg-red-500';
		if (db >= 3) return 'bg-amber-500';
		return 'bg-emerald-500';
	}

	let reading = $derived(gr.db > 0.2 ? `-${gr.db.toFixed(1)}` : '0.0');
</script>

{#if horizontal}
	<div class="flex w-full items-center gap-1.5 select-none">
		<span class="text-[9px] leading-none text-neutral-600">GR</span>
		<div class="relative h-1.5 flex-1 overflow-hidden rounded-full bg-(--lv-1)">
			<div class={['absolute inset-y-0 left-0 rounded-full', fill(gr.db)]} style="width: {pct(gr.db)}%;"></div>
			{#each ticks as t (t)}
				<div class="absolute inset-y-0 w-px bg-neutral-100" style="left: {pct(t)}%;"></div>
			{/each}
		</div>
		<span class="w-8 text-right font-mono text-[9px] leading-none text-neutral-800 tabular-nums">{reading}</span>
	</div>
{:else}
	<div class="flex flex-col items-center gap-1 select-none">
		<span class="text-[9px] leading-none text-neutral-600">GR</span>
		<div class="relative h-24 w-1.5 overflow-hidden rounded-full bg-(--lv-1)">
			<div class={['absolute inset-x-0 top-0 rounded-full', fill(gr.db)]} style="height: {pct(gr.db)}%;"></div>
			{#each ticks as t (t)}
				<div class="absolute inset-x-0 h-px bg-neutral-100" style="top: {pct(t)}%;"></div>
			{/each}
		</div>
		<span class="font-mono text-[9px] leading-none text-neutral-800 tabular-nums">{reading}</span>
	</div>
{/if}
