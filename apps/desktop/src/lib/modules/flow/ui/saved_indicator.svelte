<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { pipelineStore } from '$lib/modules/pipeline/stores.svelte';
	import Checkmark from '~icons/fluent/checkmark-16-filled';
	import { relativeTime } from '$lib/utils/time';

	let now = $state(Date.now());
	let timer: ReturnType<typeof setInterval> | undefined;
	onMount(() => {
		timer = setInterval(() => (now = Date.now()), 1000);
	});
	onDestroy(() => {
		if (timer !== undefined) clearInterval(timer);
	});

	let label = $derived.by(() => {
		now;
		const ms = pipelineStore.lastSavedAt;
		if (ms === 0) return '—';
		if (Date.now() - ms < 2000) return 'just now';
		return relativeTime(ms);
	});
</script>

<div class="flex items-center gap-1.5 text-xs text-neutral-900">
	<Checkmark class="size-3.5 text-emerald-600" />
	<span class="tabular-nums">Saved {label}</span>
</div>
