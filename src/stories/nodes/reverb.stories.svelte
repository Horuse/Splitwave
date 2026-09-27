<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				roomSize: { control: range(0, 1, 0.01) },
				damping: { control: range(0, 1, 0.01) },
				width: { control: range(0, 1, 0.01) },
				mix: { control: range(0, 1, 0.01) },
				bypassed: { control: 'boolean' }
			},
			{ roomSize: 0.5, damping: 0.5, width: 1, mix: 0.33, bypassed: false }
		),
		wiringArgs(2)
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Reverb', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="reverb" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Small room" args={{ roomSize: 0.15, damping: 0.8, mix: 0.2 }} {template} />
<Story name="Hall" args={{ roomSize: 0.95, damping: 0.2, mix: 0.6 }} {template} />
<Story name="Mono" args={{ width: 0 }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
