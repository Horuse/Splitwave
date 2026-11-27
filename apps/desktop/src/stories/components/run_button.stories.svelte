<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { fn } from 'storybook/test';
	import { RunButton } from '$lib/modules/audio/ui';

	const { Story } = defineMeta({
		title: 'Components/Run Button',
		component: RunButton,
		argTypes: {
			state: { control: 'select', options: ['run', 'stop', 'switch'] },
			size: { control: 'select', options: ['md', 'lg', 'xl'] },
			loading: { control: 'boolean' },
			disabled: { control: 'boolean' }
		},
		args: { state: 'run', size: 'lg', loading: false, disabled: false, onclick: fn() }
	});
</script>

{#snippet template(args: Record<string, any>)}
	<div class="p-8"><RunButton {...args as any} /></div>
{/snippet}

<Story name="Run" {template} />
<Story name="Stop" args={{ state: 'stop' }} {template} />
<Story name="Switch" args={{ state: 'switch' }} {template} />
<Story name="Loading" args={{ state: 'switch', loading: true }} {template} />
<Story name="Disabled" args={{ disabled: true }} {template} />
