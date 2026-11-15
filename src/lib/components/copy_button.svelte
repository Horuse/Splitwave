<script lang="ts">
	import { Button, type ButtonVariants } from '@splitwave/ui';
	import { Copy, Checkmark } from '$lib/components/icons';

	let {
		text,
		label = 'Copy',
		copiedLabel = 'Copied!',
		tone
	}: {
		text: string | (() => string);
		label?: string;
		copiedLabel?: string;
		tone?: ButtonVariants['tone'];
	} = $props();

	let copied = $state(false);
	let timer: ReturnType<typeof setTimeout> | undefined;

	async function copy() {
		try {
			await navigator.clipboard.writeText(typeof text === 'function' ? text() : text);
			copied = true;
			clearTimeout(timer);
			timer = setTimeout(() => (copied = false), 3000);
		} catch {}
	}
</script>

<Button {tone} onclick={copy}>
	{#if copied}
		<Checkmark class="size-4" />
		{copiedLabel}
	{:else}
		<Copy class="size-4" />
		{label}
	{/if}
</Button>
