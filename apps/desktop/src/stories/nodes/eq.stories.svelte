<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs({ gainsDb: { control: 'object' }, bypassed: { control: 'boolean' } }, { gainsDb: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], bypassed: false }),
		wiringArgs(2)
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/EQ', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="eq" args={a} />
{/snippet}

<Story name="Flat" {template} />
<Story name="Smile" args={{ gainsDb: [8, 6, 3, 0, -2, -2, 0, 3, 6, 8] }} {template} />
<Story name="Vocal presence" args={{ gainsDb: [-6, -4, -2, 0, 1, 3, 5, 4, 2, 0] }} {template} />
<Story name="Extremes" args={{ gainsDb: [12, -12, 12, -12, 12, -12, 12, -12, 12, -12] }} {template} />
<Story name="Bypassed" args={{ gainsDb: [8, 6, 3, 0, -2, -2, 0, 3, 6, 8], bypassed: true }} {template} />
