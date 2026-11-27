<script lang="ts">
	import { AnimatedTime } from '@splitwave/ui';

	/** Wall-clock millis to count from; zero is shown while null. */
	let { since }: { since: number | null } = $props();

	let elapsed = $state(0);

	$effect(() => {
		const start = since;
		if (!start) {
			elapsed = 0;
			return;
		}
		const tick = () => (elapsed = Math.floor((Date.now() - start) / 1000));
		tick();
		const id = setInterval(tick, 1000);
		return () => clearInterval(id);
	});
</script>

<AnimatedTime value={elapsed * 1000} />
