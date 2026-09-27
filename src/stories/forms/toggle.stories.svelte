<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { fn } from 'storybook/test';
	import Toggle from '$lib/components/toggle.svelte';
	import Controlled from '../harness/controlled.svelte';

	const { Story } = defineMeta({
		title: 'Forms/Toggle',
		argTypes: {
			checked: { control: 'boolean' },
			label: { control: 'text' },
			hint: { control: 'text' },
			disabled: { control: 'boolean' },
			size: { control: 'inline-radio', options: ['sm', 'md'] }
		},
		args: { checked: false, label: 'Exclude this app', hint: '', disabled: false, size: 'md', onChange: fn() }
	});
</script>

{#snippet template({ onChange, ...args }: Record<string, any>)}
	<div class="w-80 p-8">
		<Controlled component={Toggle} props={args} valueKey="checked" {onChange} />
	</div>
{/snippet}

<Story name="Off" {template} />
<Story name="On" args={{ checked: true }} {template} />
<Story name="With hint" args={{ hint: 'Keeps Splitwave out of its own capture' }} {template} />
<Story name="Small" args={{ size: 'sm', label: 'Loop' }} {template} />
<Story name="Disabled" args={{ disabled: true, checked: true }} {template} />
<Story name="No label" args={{ label: '' }} {template} />
