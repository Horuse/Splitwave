<script lang="ts">
	import * as Icons from '#lib/components/icons/index.js';

	let { sha256 }: { sha256: string } = $props();

	let copied = $state(false);

	async function copy() {
		try {
			await navigator.clipboard.writeText(sha256);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			copied = false;
		}
	}
</script>

<button
	type="button"
	onclick={copy}
	title={sha256}
	aria-label="Copy SHA-256 checksum"
	class="inline-flex items-center gap-1.5 text-xs text-neutral-900 transition-colors hover:text-theme"
>
	<Icons.ShieldCheckmark class="size-3.5" />
	<span>SHA-256</span>
	<span class="font-mono">{sha256.slice(0, 8)}…{sha256.slice(-8)}</span>
	{#if copied}
		<Icons.Checkmark class="size-3.5" />
	{:else}
		<Icons.Copy class="size-3.5" />
	{/if}
</button>
