<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import Eye from '~icons/fluent/eye-16-filled';
	import EyeOff from '~icons/fluent/eye-off-16-filled';

	interface Props {
		value: string;
		placeholder?: string;
		class?: ClassValue;
	}

	let { value = $bindable(''), placeholder = 'Password', class: cls }: Props = $props();

	let visible = $state(false);
</script>

<div class="relative w-full">
	<input
		type={visible ? 'text' : 'password'}
		class={[
			'nowheel h-7 w-full rounded-lg border border-theme/5 bg-(--field-bg,var(--lv-1)) pr-7 pl-2.5 font-mono text-[11px] text-neutral-1100 outline-none placeholder:text-neutral-600 focus:border-(--lv-4)',
			cls
		]}
		{placeholder}
		bind:value />
	<button
		type="button"
		tabindex={-1}
		class="absolute top-1/2 right-2 flex -translate-y-1/2 text-neutral-500 transition-colors duration-150 hover:text-neutral-800"
		title={visible ? 'Hide password' : 'Show password'}
		onclick={() => (visible = !visible)}>
		{#if visible}
			<Eye class="h-3.5 w-3.5" />
		{:else}
			<EyeOff class="h-3.5 w-3.5" />
		{/if}
	</button>
</div>
