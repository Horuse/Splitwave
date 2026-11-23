<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Toggle from './toggle.svelte';

	const { Story } = defineMeta({
		title: 'Form/Toggle',
		component: Toggle,
		tags: ['autodocs'],
		argTypes: {
			size: { control: 'select', options: ['sm', 'md'] },
			label: { control: 'text' },
			hint: { control: 'text' },
			disabled: { control: 'boolean' },
			checked: { control: 'boolean' }
		},
		args: { checked: false, label: 'Enabled', size: 'md', disabled: false }
	});
</script>

<script lang="ts">
	let on = $state(true);
</script>

<Story name="Playground">
	{#snippet template(args)}
		<div class="w-72 p-6">
			<Toggle {...args} onChange={() => {}} />
		</div>
	{/snippet}
</Story>

<Story name="Variants">
	{#snippet template()}
		<div class="flex w-72 flex-col gap-4 p-6">
			<Toggle checked={on} onChange={(v) => (on = v)} label="Medium" />
			<Toggle checked={on} onChange={(v) => (on = v)} label="Small" size="sm" />
			<Toggle checked={on} onChange={(v) => (on = v)} label="With hint" hint="Shown as a settings row" />
			<Toggle checked={on} onChange={(v) => (on = v)} label="Disabled" disabled />
		</div>
	{/snippet}
</Story>
