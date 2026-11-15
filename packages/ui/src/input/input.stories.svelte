<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Input from './input.svelte';

	const { Story } = defineMeta({
		title: 'Form/Input',
		component: Input,
		tags: ['autodocs'],
		argTypes: {
			size: { control: 'select', options: ['sm', 'base', 'md'] },
			mono: { control: 'boolean' },
			invalid: { control: 'boolean' },
			disabled: { control: 'boolean' }
		},
		args: { placeholder: 'Pipeline name', size: 'base', mono: false, invalid: false, disabled: false }
	});
</script>

<Story name="Playground">
	{#snippet template(args)}
		<div class="w-64 p-6">
			<Input {...args} />
		</div>
	{/snippet}
</Story>

<Story name="Variants">
	{#snippet template()}
		<div class="flex w-64 flex-col gap-3 p-6">
			<Input size="sm" placeholder="Small" />
			<Input placeholder="Base" />
			<Input size="md" placeholder="Medium" />
			<Input mono value="-12.4 dB" />
			<Input invalid value="Invalid" />
			<Input disabled value="Disabled" />
		</div>
	{/snippet}
</Story>
