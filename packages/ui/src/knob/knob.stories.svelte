<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import Knob from './knob.svelte';

	const { Story } = defineMeta({ title: 'Form/Knob', component: Knob, tags: ['autodocs'] });
</script>

<script lang="ts">
	let gain = $state(0);
	let mix = $state(0.4);
	let freq = $state(1000);
</script>

<Story name="Row">
	{#snippet template()}
		<div class="grid w-64 grid-cols-3 gap-2 p-6 text-violet-500">
			<Knob label="Gain" value={gain} min={-24} max={24} step={0.1} defaultValue={0} format={(v) => `${v.toFixed(1)} dB`} onChange={(v) => (gain = v)} />
			<Knob label="Mix" value={mix} min={0} max={1} defaultValue={1} format={(v) => `${Math.round(v * 100)}%`} onChange={(v) => (mix = v)} />
			<Knob label="Frequency" value={freq} min={20} max={20000} step={1} format={(v) => `${Math.round(v)} Hz`} onChange={(v) => (freq = v)} />
		</div>
	{/snippet}
</Story>
