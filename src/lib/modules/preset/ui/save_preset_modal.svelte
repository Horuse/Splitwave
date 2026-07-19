<script lang="ts">
	import { modalManager, type ModalBaseProps } from '$lib/modules/overlay/modal';

	let { modalId, taken = [] }: ModalBaseProps & { taken?: string[] } = $props();

	let name = $state('');
	let clash = $derived(taken.some((t) => t.toLowerCase() === name.trim().toLowerCase()));
	let valid = $derived(name.trim().length > 0 && !clash);

	function submit() {
		if (!valid) return;
		modalManager.close(modalId, name.trim());
	}
</script>

<form
	class="flex w-80 flex-col gap-4 p-5"
	onsubmit={(e) => {
		e.preventDefault();
		submit();
	}}
>
	<div class="flex flex-col gap-1.5">
		<label for="preset-name" class="text-xs text-neutral-1000">Preset name</label>
		<!-- svelte-ignore a11y_autofocus -->
		<input
			id="preset-name"
			bind:value={name}
			autofocus
			placeholder="Vocal chain"
			class="input-base w-full"
		/>
		{#if clash}
			<span class="text-[11px] text-red-500">A preset with this name already exists.</span>
		{/if}
	</div>

	<div class="flex justify-end gap-2">
		<button
			type="button"
			class="button-main rounded-lg"
			onclick={() => modalManager.close(modalId, undefined)}
		>
			Cancel
		</button>
		<button type="submit" class="button-main primary rounded-lg" disabled={!valid}>Save</button>
	</div>
</form>
