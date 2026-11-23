<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NumberStepper from './number-stepper.svelte';

	const { Story } = defineMeta({
		title: 'Form/Number Stepper',
		component: NumberStepper,
		tags: ['autodocs']
	});
</script>

<script lang="ts">
	let value = $state(4);
</script>

<Story name="Default">
	{#snippet template()}
		<div class="p-6">
			<NumberStepper {value} min={1} max={16} label="Channels" onchange={(v) => (value = v)} />
		</div>
	{/snippet}
</Story>

<Story name="Disabled">
	{#snippet template()}
		<div class="p-6">
			<NumberStepper value={4} min={1} max={16} disabled onchange={() => {}} />
		</div>
	{/snippet}
</Story>
