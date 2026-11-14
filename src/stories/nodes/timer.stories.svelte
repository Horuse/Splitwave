<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				mode: { control: 'inline-radio', options: ['delay', 'repeat'] },
				ms: { control: { type: 'range', min: 50, max: 10000, step: 50 } }
			},
			{ mode: 'delay', ms: 1000 }
		)
	);

	const { Story } = defineMeta({ title: 'Nodes/Conditions/Timer', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="timer" args={a} />
{/snippet}

<Story name="Delay" {template} />
<Story name="Repeat" args={{ mode: 'repeat', ms: 500 }} {template} />
