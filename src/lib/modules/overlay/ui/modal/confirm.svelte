<script lang="ts">
	import { modalManager, type ModalBaseProps } from '../../modal';

	interface Props {
		message?: string;
		confirmLabel?: string;
		cancelLabel?: string;
		danger?: boolean;
		/** When set, shows a checkbox; the resolved value becomes
		 * `{ ok, dontAskAgain }` instead of a plain boolean. */
		checkboxLabel?: string;
	}

	let {
		modalId,
		message = 'Are you sure?',
		confirmLabel = 'Confirm',
		cancelLabel = 'Cancel',
		danger = false,
		checkboxLabel
	}: ModalBaseProps & Props = $props();

	let dontAskAgain = $state(false);

	function close(ok: boolean) {
		modalManager.close(modalId, checkboxLabel !== undefined ? { ok, dontAskAgain } : ok);
	}
</script>

<div class="flex flex-col gap-4 px-5 py-4">
	<p class="text-sm text-neutral-1100">{message}</p>

	{#if checkboxLabel}
		<label class="flex cursor-pointer items-center gap-2 text-xs text-neutral-900 select-none">
			<input type="checkbox" bind:checked={dontAskAgain} class="size-3.5 accent-neutral-800" />
			{checkboxLabel}
		</label>
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
