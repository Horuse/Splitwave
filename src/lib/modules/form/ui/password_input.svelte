<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import { Eye, EyeOff } from '$lib/components/icons';

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
			'nowheel h-6 w-full rounded border border-neutral-300 bg-neutral-50 pl-1.5 pr-6 font-mono text-[10px] text-neutral-800 placeholder:text-neutral-400',
			cls
		]}
		{placeholder}
		bind:value
	/>
	<button
		type="button"
		tabindex={-1}
		class="absolute right-1 top-1/2 -translate-y-1/2 text-neutral-500 hover:text-neutral-800"
		title={visible ? 'Hide password' : 'Show password'}
		onclick={() => (visible = !visible)}
	>
		{#if visible}
			<Eye class="h-3.5 w-3.5" />
		{:else}
			<EyeOff class="h-3.5 w-3.5" />
		{/if}
	</button>
</div>
