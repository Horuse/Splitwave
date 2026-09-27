<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{ leftGainDb: { control: range(-24, 24, 0.1) }, rightGainDb: { control: range(-24, 24, 0.1) }, bypassed: { control: 'boolean' } },
			{ leftGainDb: 0, rightGainDb: 0, bypassed: false }
		),
		wiringArgs(2)
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Channel Balance', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="channelBalance" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Left heavy" args={{ leftGainDb: 6, rightGainDb: -12 }} {template} />
<Story name="Right heavy" args={{ leftGainDb: -12, rightGainDb: 6 }} {template} />
<Story name="Extremes" args={{ leftGainDb: -24, rightGainDb: 24 }} {template} />
<Story name="Bypassed" args={{ leftGainDb: 6, bypassed: true }} {template} />
