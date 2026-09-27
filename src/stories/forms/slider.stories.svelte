<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { fn } from 'storybook/test';
	import Slider from '$lib/modules/flow/ui/effect/_slider.svelte';
	import Controlled from '../harness/controlled.svelte';

	const FORMATS = {
		none: undefined,
		ratio: (v: number) => `${v.toFixed(1)}:1`,
		percent: (v: number) => `${Math.round(v * 100)}%`
	};

	const { Story } = defineMeta({
		title: 'Forms/Slider',
		argTypes: {
			label: { control: 'text' },
			value: { control: 'number' },
			min: { control: 'number' },
			max: { control: 'number' },
			step: { control: 'number' },
			unit: { control: 'text' },
			defaultValue: { control: 'number' },
			ticks: { control: 'object' },
			valueClass: { control: 'select', options: ['text-neutral-900', 'text-emerald-700', 'text-amber-600', 'text-red-500'] },
			format: { control: 'select', options: Object.keys(FORMATS), mapping: FORMATS }
		},
		args: {
			label: 'Level',
			value: 0,
			min: -24,
			max: 24,
			step: 0.1,
			unit: 'dB',
			defaultValue: 0,
			valueClass: 'text-emerald-700',
			format: 'none',
			onChange: fn()
		}
	});
</script>

{#snippet template({ onChange, ...args }: Record<string, any>)}
	<div class="m-8 w-72 rounded-2xl border border-neutral-400 bg-neutral-200 p-4">
		<Controlled component={Slider} props={args} {onChange} />
	</div>
{/snippet}

<Story name="Decibels" {template} />
<Story name="With ticks" args={{ ticks: [-12, -6, 0, 6, 12] }} {template} />
<Story
	name="Custom format"
	args={{ label: 'Ratio', value: 4, min: 1, max: 20, unit: '', defaultValue: 3, format: 'ratio', valueClass: 'text-neutral-900' }}
	{template} />
<Story
	name="Percent"
	args={{ label: 'Mix', value: 0.35, min: 0, max: 1, step: 0.01, unit: '', format: 'percent', valueClass: 'text-neutral-900' }}
	{template} />
<Story name="Warning colour" args={{ value: 18, valueClass: 'text-red-500' }} {template} />
<Story name="At bounds" args={{ value: 24 }} {template} />
