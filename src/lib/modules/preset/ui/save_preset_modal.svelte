<script lang="ts">
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';
	import { t } from '$lib/modules/i18n';

	let { modalId, taken = [] }: ModalBaseProps & { taken?: string[] } = $props();

	let name = $state('');
	let trimmed = $derived(name.trim());
	let clash = $derived(taken.some((n) => n.toLowerCase() === trimmed.toLowerCase()));
	let valid = $derived(trimmed.length > 0 && !clash);

	function submit() {
		if (!valid) return;
		modalManager.close(modalId, trimmed);
	}
</script>

<form
	class="flex flex-col"
	onsubmit={(e) => {
		e.preventDefault();
		submit();
	}}>
	<div class="flex flex-col gap-1.5 px-5 py-4">
		<!-- Not `input-base`: its background matches the modal card, and it centres
		     its text for compact fields inside nodes. -->
		<!-- svelte-ignore a11y_autofocus -->
		<input
			bind:value={name}
			autofocus
			placeholder={t('preset.namePlaceholder')}
			aria-label={t('preset.nameAria')}
			class="w-full rounded-lg border border-neutral-400 bg-neutral-200 px-3 py-2 text-sm text-theme transition-colors placeholder:text-neutral-900 focus:border-neutral-600 focus:outline-none" />
		<span class={['text-xs', clash ? 'text-red-500' : 'text-neutral-900']}>
			{clash ? t('preset.nameTaken') : t('preset.nameAvailable')}
		</span>
	</div>

	<div class="flex justify-end gap-2 px-5 pb-4">
		<button type="button" class="button-main primary rounded-lg" onclick={() => modalManager.close(modalId, undefined)}>
			{t('common.cancel')}
		</button>
		<button type="submit" class="button-main green rounded-lg" disabled={!valid}>{t('common.save')}</button>
	</div>
</form>
