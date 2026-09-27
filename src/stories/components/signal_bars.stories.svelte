<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import SignalBars from '$lib/components/signal_bars.svelte';

	const { Story } = defineMeta({
		title: 'Components/Signal Bars',
		argTypes: {
			loss: { control: { type: 'range', min: 0, max: 0.3, step: 0.005 } },
			ping: { control: { type: 'range', min: 0, max: 500, step: 5 } },
			bars: { control: { type: 'range', min: 2, max: 6, step: 1 } }
		},
		args: { loss: 0, ping: 24, bars: 4 }
	});
</script>

{#snippet template(args: Record<string, any>)}
	<div class="p-8"><SignalBars {...args} /></div>
{/snippet}

<Story name="Good" {template} />
<Story name="Fair" args={{ loss: 0.05, ping: 150 }} {template} />
<Story name="Poor" args={{ loss: 0.25, ping: 400 }} {template} />
<Story name="Unknown" args={{ loss: null, ping: null }} {template} />
