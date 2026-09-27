<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { fn } from 'storybook/test';
	import SegmentedButtons from '$lib/components/segmented_buttons.svelte';
	import Controlled from '../harness/controlled.svelte';

	const SETS = {
		codecs: [
			{ value: 'opus', label: 'Opus', subtitle: 'compressed' },
			{ value: 'pcm-f32', label: 'PCM', subtitle: 'f32' },
			{ value: 'pcm-i16', label: 'PCM', subtitle: 'i16' }
		],
		modes: [
			{ value: 'new', label: 'New' },
			{ value: 'overwrite', label: 'Overwrite' },
			{ value: 'append', label: 'Append', disabled: true }
		],
		rates: ['auto', '44100', '48000', '88200', '96000', 'custom'].map((v) => ({
			value: v,
			label: v === 'auto' || v === 'custom' ? v[0].toUpperCase() + v.slice(1) : `${Number(v) / 1000}k`
		}))
	};

	const { Story } = defineMeta({
		title: 'Forms/Segmented Buttons',
		argTypes: {
			options: { control: 'select', options: Object.keys(SETS), mapping: SETS },
			value: { control: 'text' },
			label: { control: 'text' },
			note: { control: 'text' },
			columns: { control: { type: 'range', min: 1, max: 6, step: 1 } }
		},
		args: { options: 'codecs', value: 'opus', label: 'Codec', note: '', onSelect: fn() }
	});
</script>

{#snippet template({ onSelect, ...args }: Record<string, any>)}
	<div class="w-72 p-8">
		<Controlled component={SegmentedButtons} props={args} changeKey="onSelect" onChange={onSelect} />
	</div>
{/snippet}

<Story name="With subtitles" {template} />
<Story name="Disabled option" args={{ options: 'modes', value: 'new', label: 'Mode' }} {template} />
<Story name="Grid" args={{ options: 'rates', value: '48000', label: 'Sample rate', columns: 3 }} {template} />
<Story name="With note" args={{ note: 'Opus always runs at 48 kHz' }} {template} />
<Story name="No label" args={{ label: '' }} {template} />
