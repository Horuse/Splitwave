<script lang="ts">
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';

	let {
		modalId,
		version = '',
		current = ''
	}: ModalBaseProps & { version?: string; current?: string } = $props();
</script>

<div class="flex flex-col gap-4 p-5">
	<div class="warning-block">
		<span class="font-semibold">
			{current} to {version} is a major update and may break saved pipelines.
		</span>
		<span>
			Pipelines saved by the current version will be marked as outdated: they stay on disk and can
			be deleted, but cannot be opened or run. Their routing cannot be carried across safely, so
			nothing is rewritten behind your back.
		</span>
	</div>

	<p class="text-xs text-neutral-1000">
		Screenshot any pipeline you rely on before updating, so you can rebuild it from the picture.
		Downgrading afterwards is not supported.
	</p>

	<div class="flex justify-end gap-2">
		<button
			type="button"
			class="button-main primary rounded-lg"
			onclick={() => modalManager.close(modalId, false)}
		>
			Not now
		</button>
		<button
			type="button"
			class="button-main red rounded-lg"
			onclick={() => modalManager.close(modalId, true)}
		>
			I understand, update
		</button>
	</div>
</div>
