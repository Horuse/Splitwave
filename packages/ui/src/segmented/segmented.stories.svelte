<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Segmented from './segmented.svelte';

	const { Story } = defineMeta({
		title: 'Form/Segmented',
		component: Segmented,
		tags: ['autodocs']
	});

	const options = [
		{ label: '44.1k', value: 44100 },
		{ label: '48k', value: 48000 },
		{ label: '96k', value: 96000, subtitle: 'hi-res' },
		{ label: '192k', value: 192000, disabled: true }
	];
</script>

<script lang="ts">
	let value = $state(48000);
</script>

<Story name="Single row">
	{#snippet template()}
		<div class="w-72 p-6">
			<Segmented {options} {value} onSelect={(v) => (value = v)} label="Sample rate" note="Hz" />
		</div>
	{/snippet}
</Story>

<Story name="Grid">
	{#snippet template()}
		<div class="w-48 p-6">
			<Segmented {options} {value} columns={2} onSelect={(v) => (value = v)} />
		</div>
	{/snippet}
</Story>
