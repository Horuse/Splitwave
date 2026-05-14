<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import { Checkmark } from '$lib/components/icons';

	let now = $state(Date.now());
	let timer: ReturnType<typeof setInterval> | undefined;
	onMount(() => {
		timer = setInterval(() => (now = Date.now()), 1000);
	});
	onDestroy(() => {
		if (timer !== undefined) clearInterval(timer);
	});

	function format(ms: number): string {
		if (ms === 0) return '—';
		const diff = now - ms;
		if (diff < 2000) return 'just now';
		if (diff < 60_000) return `${Math.floor(diff / 1000)}s ago`;
		if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
		const d = new Date(ms);
		return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}`;
	}
</script>

<div class="flex items-center gap-1 text-[10px] text-neutral-900">
	<Checkmark class="h-3 w-3 text-green-600" />
	<span class="font-mono tabular-nums">Saved {format(pipelineStore.lastSavedAt)}</span>
</div>
