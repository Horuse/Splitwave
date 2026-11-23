<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Slider from './slider.svelte';

	const { Story } = defineMeta({
		title: 'Form/Slider',
		component: Slider,
		tags: ['autodocs'],
		argTypes: {
			tone: { control: 'select', options: ['default', 'success', 'warning', 'danger'] }
		}
	});
</script>

<script lang="ts">
	let gain = $state(0);
	let mix = $state(0.35);
	let hot = $state(18);
</script>

<Story name="Decibels">
	{#snippet template()}
		<div class="w-72 p-6">
			<Slider
				label="Level"
				value={gain}
				min={-24}
				max={24}
				step={0.1}
				unit=" dB"
				defaultValue={0}
				ticks={[-12, -6, 0, 6, 12]}
				onChange={(v) => (gain = v)} />
		</div>
	{/snippet}
</Story>

<Story name="Percent">
	{#snippet template()}
		<div class="w-72 p-6">
			<Slider label="Mix" value={mix} min={0} max={1} step={0.01} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => (mix = v)} />
		</div>
	{/snippet}
</Story>

<Story name="Warning tone">
	{#snippet template()}
		<div class="w-72 p-6">
			<Slider label="Level" value={hot} min={-24} max={24} step={0.1} unit=" dB" tone="danger" onChange={(v) => (hot = v)} />
		</div>
	{/snippet}
</Story>
