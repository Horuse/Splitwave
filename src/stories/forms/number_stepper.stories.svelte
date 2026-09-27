<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { fn } from 'storybook/test';
	import NumberStepper from '$lib/components/number_stepper.svelte';
	import Controlled from '../harness/controlled.svelte';

	const { Story } = defineMeta({
		title: 'Forms/Number Stepper',
		argTypes: {
			value: { control: 'number' },
			min: { control: 'number' },
			max: { control: 'number' },
			step: { control: 'number' },
			disabled: { control: 'boolean' },
			label: { control: 'text' },
			width: { control: 'inline-radio', options: ['w-12', 'w-16', 'w-24'] }
		},
		args: { value: 2, min: 1, max: 16, step: 1, disabled: false, label: 'Channels', width: 'w-12', onchange: fn() }
	});
</script>

{#snippet template({ onchange, ...args }: Record<string, any>)}
	<div class="w-fit p-8">
		<Controlled component={NumberStepper} props={args} changeKey="onchange" onChange={onchange} />
	</div>
{/snippet}

<Story name="Default" {template} />
<Story name="At minimum" args={{ value: 1 }} {template} />
<Story name="At maximum" args={{ value: 16 }} {template} />
<Story name="Large step" args={{ value: 48000, min: 8000, max: 384000, step: 1000, width: 'w-24', label: 'Sample rate' }} {template} />
<Story name="Disabled" args={{ disabled: true }} {template} />
