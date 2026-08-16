<script lang="ts">
	import Gauge from '$lib/components/icons/gauge.svelte';
	import { methods } from '../methods';
	import { audioStore } from '../stores.svelte';

	let latencyMs = $state<number | null>(null);

	$effect(() => {
		if (!audioStore.isRunning) {
			latencyMs = null;
			return;
		}
		let cancelled = false;
		const poll = async () => {
			const v = await methods.getOutputLatency().catch(() => null);
			if (!cancelled) latencyMs = v;
		};
		void poll();
		const id = setInterval(() => void poll(), 500);
		return () => {
			cancelled = true;
			clearInterval(id);
		};
	});
</script>

{#if latencyMs !== null && latencyMs > 0}
	<span
		class="flex items-center gap-1.5 rounded-md border border-theme/10 bg-background px-2 py-0.5"
		title="Total latency (input + all nodes + output buffer)">
		<Gauge class="h-3.5 w-3.5 text-neutral-500" />
		<span class="font-mono text-xs text-neutral-800 tabular-nums">{latencyMs} ms</span>
	</span>
{/if}
