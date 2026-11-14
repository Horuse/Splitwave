<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				steps: { control: { type: 'range', min: 2, max: 8, step: 1 } },
				step: { control: { type: 'range', min: 1, max: 8, step: 1 } }
			},
			{ steps: 3, step: 1 }
		)
	);

	const { Story } = defineMeta({ title: 'Nodes/Conditions/Cycle', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="cycle" args={a} />
{/snippet}

<Story name="Three steps" {template} />
<Story name="On the third" args={{ step: 3 }} {template} />
<Story name="Eight steps" args={{ steps: 8, step: 5 }} {template} />
