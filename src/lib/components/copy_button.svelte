<script lang="ts">
	import { Copy, Checkmark } from '$lib/components/icons';
	import { t } from '$lib/modules/i18n';

	let {
		text,
		label,
		copiedLabel,
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
		} catch {}
	}
</script>

<button type="button" class={className} onclick={copy}>
	{#if copied}
		<Checkmark class="size-4" />
		{copiedLabel ?? t('common.copied')}
	{:else}
		<Copy class="size-4" />
		{label ?? t('common.copy')}
	{/if}
</button>
