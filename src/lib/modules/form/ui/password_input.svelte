<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import { Eye, EyeOff } from '$lib/components/icons';
	import { t } from '$lib/modules/i18n';

	interface Props {
		value: string;
		placeholder?: string;
		class?: ClassValue;
	}

	let { value = $bindable(''), placeholder, class: cls }: Props = $props();

	let placeholderText = $derived(placeholder ?? t('form.password'));

	let visible = $state(false);
</script>

<div class="relative w-full">
	<input
		type={visible ? 'text' : 'password'}
		class={[
			'nowheel h-6 w-full rounded border border-neutral-300 bg-neutral-50 pr-6 pl-1.5 font-mono text-[10px] text-neutral-800 placeholder:text-neutral-400',
			cls
		]}
		placeholder={placeholderText}
		bind:value />
	<button
		type="button"
		tabindex={-1}
		class="absolute top-1/2 right-1 flex -translate-y-1/2 text-neutral-500 hover:text-neutral-800"
		title={visible ? t('form.hidePassword') : t('form.showPassword')}
		onclick={() => (visible = !visible)}>
		{#if visible}
			<Eye class="h-3.5 w-3.5" />
		{:else}
			<EyeOff class="h-3.5 w-3.5" />
		{/if}
	</button>
</div>
