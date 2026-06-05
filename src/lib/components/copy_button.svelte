<script lang="ts">
	import { Copy, Checkmark } from '$lib/components/icons';

	let {
		text,
		label = 'Copy',
		copiedLabel = 'Copied!',
		class: className = 'button-main primary gap-3 rounded-lg'
	}: {
		text: string | (() => string);
		label?: string;
		copiedLabel?: string;
		class?: string;
	} = $props();

	let copied = $state(false);
	let timer: ReturnType<typeof setTimeout> | undefined;

	async function copy() {
		try {
			await navigator.clipboard.writeText(typeof text === 'function' ? text() : text);
			copied = true;
			clearTimeout(timer);
			timer = setTimeout(() => (copied = false), 3000);
		} catch {
		}
	}
</script>

<button type="button" class={className} onclick={copy}>
	{#if copied}
		<Checkmark class="size-4" />
		{copiedLabel}
	{:else}
		<Copy class="size-4" />
		{label}
	{/if}
</button>
