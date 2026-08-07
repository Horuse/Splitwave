<script lang="ts">
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onMount, onDestroy } from 'svelte';

	let { nodeId, horizontal = false }: { nodeId: string; horizontal?: boolean } = $props();

	const GR_MAX = 20;
	const FALL_DB_PER_SEC = 18;

	let targetGrDb = 0;
	let displayGrDb = $state(0);

	let unlisten: UnlistenFn | undefined;
	let rafId: number | undefined;
	let lastFrame = 0;

	function tick(now: number) {
		const dt = lastFrame ? Math.min((now - lastFrame) / 1000, 0.1) : 0;
		lastFrame = now;
		if (targetGrDb > displayGrDb) {
			displayGrDb = targetGrDb;
		} else {
			displayGrDb = Math.max(targetGrDb, displayGrDb - FALL_DB_PER_SEC * dt);
		}
		rafId = requestAnimationFrame(tick);
	}

	onMount(async () => {
		unlisten = await listen<{ nodeId: string; grLin: number }>('audio://gr', (event) => {
			const p = event.payload;
			if (p.nodeId !== nodeId) return;
			const lin = Math.max(1e-6, Math.min(1, p.grLin));
			targetGrDb = -20 * Math.log10(lin);
		});
		rafId = requestAnimationFrame(tick);
	});

	onDestroy(() => {
		unlisten?.();
		if (rafId) cancelAnimationFrame(rafId);
	});

	function grToPct(db: number): number {
		return Math.min(100, Math.max(0, (db / GR_MAX) * 100));
	}

	function grColor(db: number): string {
		if (db >= 6) return '#ef4444';
		if (db >= 3) return '#eab308';
		return '#22c55e';
	}

	const ticks = [3, 6, 12, 20];
</script>

{#if horizontal}
	<div class="flex w-full items-center gap-1 select-none">
		<span class="w-5 shrink-0 text-[8px] leading-none text-neutral-400">GR</span>
		<div class="relative h-2.5 flex-1 overflow-hidden rounded-sm border border-neutral-300 bg-neutral-100">
			<div
				class="absolute top-0 bottom-0 left-0"
				style="width: {grToPct(displayGrDb)}%; background: {grColor(displayGrDb)}; transition: background 0.1s;">
			</div>
			{#each ticks as t}
				<div class="absolute top-0 bottom-0 w-px bg-neutral-300/60" style="left: {grToPct(t)}%;"></div>
			{/each}
		</div>
		<span class="w-7 shrink-0 text-right font-mono text-[8px] leading-none text-neutral-500 tabular-nums">
			{displayGrDb > 0.2 ? `-${displayGrDb.toFixed(1)}` : '0'}
		</span>
	</div>
{:else}
	<div class="flex flex-col items-center gap-0.5 select-none">
		<span class="text-[8px] leading-none text-neutral-400">GR</span>
		<div class="relative h-24 w-3 overflow-hidden rounded-sm border border-neutral-300 bg-neutral-100">
			<div
				class="absolute top-0 right-0 left-0"
				style="height: {grToPct(displayGrDb)}%; background: {grColor(displayGrDb)}; transition: background 0.1s;">
			</div>
			{#each ticks as t}
				<div class="absolute right-0 left-0 h-px bg-neutral-300/60" style="top: {grToPct(t)}%;"></div>
			{/each}
		</div>
		<span class="w-7 text-center font-mono text-[8px] leading-none text-neutral-500 tabular-nums">
			{displayGrDb > 0.2 ? `-${displayGrDb.toFixed(1)}` : '0'}
		</span>
	</div>
{/if}
