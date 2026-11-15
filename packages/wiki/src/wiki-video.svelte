<script lang="ts">
	let { mp4, webm }: { mp4: string; webm?: string } = $props();
</script>

<video autoplay loop muted playsinline class="my-4 w-full rounded-xl border border-neutral-300 bg-neutral-100">
	<source src={mp4} type="video/mp4" />
	{#if webm}<source src={webm} type="video/webm" />{/if}
</video>
