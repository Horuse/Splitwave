<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import PasswordInput from './password-input.svelte';

	const { Story } = defineMeta({ title: 'Form/Password Input', component: PasswordInput, tags: ['autodocs'] });
</script>

<script lang="ts">
	let value = $state('hunter2');
</script>

<Story name="Default">
	{#snippet template()}
		<div class="w-64 p-6"><PasswordInput bind:value /></div>
	{/snippet}
</Story>
