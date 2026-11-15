<script lang="ts">
	import type { ClassValue } from 'svelte/elements';
	import IconEye from '~icons/fluent/eye-16-filled';
	import IconEyeOff from '~icons/fluent/eye-off-16-filled';
	import { Input } from '../input';

	let { value = $bindable(''), placeholder = 'Password', class: className }: { value: string; placeholder?: string; class?: ClassValue } = $props();

	let visible = $state(false);
</script>

<div class="relative w-full">
	<Input type={visible ? 'text' : 'password'} mono class={['w-full pr-6', className]} {placeholder} bind:value />
	<button
		type="button"
		tabindex={-1}
		class="absolute top-1/2 right-1 flex -translate-y-1/2 text-muted hover:text-content"
		title={visible ? 'Hide password' : 'Show password'}
		onclick={() => (visible = !visible)}>
		{#if visible}
			<IconEye class="size-3.5" />
		{:else}
			<IconEyeOff class="size-3.5" />
		{/if}
	</button>
</div>
