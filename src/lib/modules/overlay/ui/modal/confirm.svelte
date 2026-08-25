<script lang="ts">
	import { modalManager, type ModalBaseProps } from '../../modal';
	import Toggle from '$lib/components/toggle.svelte';

	interface Props {
		message?: string;
		confirmLabel?: string;
		cancelLabel?: string;
		danger?: boolean;
		/** When set, shows a checkbox; the resolved value becomes
		 * `{ ok, dontAskAgain }` instead of a plain boolean. */
		checkboxLabel?: string;
		/** Extra emphasis shown between the message and the actions, e.g. a
		 * warning that the change takes effect on the running pipeline. */
		warning?: string;
	}

	let {
		modalId,
		message = 'Are you sure?',
		confirmLabel = 'Confirm',
		cancelLabel = 'Cancel',
		danger = false,
		checkboxLabel,
		warning
	}: ModalBaseProps & Props = $props();

	let dontAskAgain = $state(false);

	function close(ok: boolean) {
		modalManager.close(modalId, checkboxLabel !== undefined ? { ok, dontAskAgain } : ok);
	}
</script>

<div class="flex flex-col gap-4 px-5 py-4">
	<p class="text-sm text-neutral-1100">{message}</p>

	{#if warning}
		<div class="warning-block">{warning}</div>
	{/if}

	{#if checkboxLabel}
		<Toggle checked={dontAskAgain} label={checkboxLabel} onChange={(v) => (dontAskAgain = v)} />
	{/if}

	<div class="flex justify-end gap-2">
		<button type="button" class="button-main primary rounded-lg" onclick={() => close(false)}>
			{cancelLabel}
		</button>
		<button type="button" class={['button-main rounded-lg', danger ? 'red' : 'green']} onclick={() => close(true)}>
			{confirmLabel}
		</button>
	</div>
</div>
