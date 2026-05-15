<script lang="ts">
	import { modalManager, type ModalBaseProps } from '../../modal';

	interface Props {
		message?: string;
		confirmLabel?: string;
		cancelLabel?: string;
		danger?: boolean;
	}

	let {
		modalId,
		message = 'Are you sure?',
		confirmLabel = 'Confirm',
		cancelLabel = 'Cancel',
		danger = false
	}: ModalBaseProps & Props = $props();
</script>

<div class="flex flex-col gap-4 p-5">
	<p class="text-sm text-neutral-1100">{message}</p>

	<div class="flex justify-end gap-2">
		<button
			type="button"
			class="button-main primary rounded-lg"
			onclick={() => modalManager.close(modalId, false)}
		>
			{cancelLabel}
		</button>
		<button
			type="button"
			class={['button-main rounded-lg', danger ? 'red' : 'green']}
			onclick={() => modalManager.close(modalId, true)}
		>
			{confirmLabel}
		</button>
	</div>
</div>
