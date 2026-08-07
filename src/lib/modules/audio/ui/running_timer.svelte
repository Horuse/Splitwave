<script lang="ts">
	import { audioStore } from '../stores.svelte';

	let elapsed = $state(0);

	$effect(() => {
		if (!audioStore.startedAt) {
			elapsed = 0;
			return;
		}
		elapsed = Math.floor((Date.now() - audioStore.startedAt) / 1000);
		const id = setInterval(() => {
			if (!audioStore.startedAt) return;
			elapsed = Math.floor((Date.now() - audioStore.startedAt) / 1000);
		}, 1000);
		return () => clearInterval(id);
	});

	function format(s: number): string {
		const h = Math.floor(s / 3600);
		const m = Math.floor((s % 3600) / 60);
		const sec = s % 60;
		if (h > 0) return `${h}:${m.toString().padStart(2, '0')}:${sec.toString().padStart(2, '0')}`;
		return `${m}:${sec.toString().padStart(2, '0')}`;
	}
</script>

<span class="font-mono text-xs text-neutral-800 tabular-nums">{format(elapsed)}</span>
