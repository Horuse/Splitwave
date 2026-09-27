<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				frequency: { control: range(2000, 16000, 50) },
				thresholdDb: { control: range(-80, 0, 0.5) },
				ratio: { control: range(1, 12, 0.1) },
				bypassed: { control: 'boolean' }
			},
			{ frequency: 6500, thresholdDb: -30, ratio: 4, bypassed: false }
		),
		wiringArgs(2)
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/De-esser', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="deEsser" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Harsh vocal" args={{ frequency: 8000, thresholdDb: -45, ratio: 10 }} {template} />
<Story name="Low split" args={{ frequency: 2000 }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
