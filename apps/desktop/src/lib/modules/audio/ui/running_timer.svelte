<script lang="ts">
	import { audioStore } from '../stores.svelte';
	import { AnimatedTime } from '@splitwave/ui';

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
</script>

<AnimatedTime value={elapsed * 1000} />
