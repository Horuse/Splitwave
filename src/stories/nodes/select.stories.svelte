<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range } from '../harness/args';

	const { argTypes, args } = compose(dataArgs({ inputs: { control: range(2, 8, 1) }, selected: { control: range(1, 8, 1) } }, { inputs: 3, selected: 1 }));

	const { Story } = defineMeta({ title: 'Nodes/Conditions/Select', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="select" args={a} />
{/snippet}

<Story name="Three inputs" {template} />
<Story name="Second heard" args={{ selected: 2 }} {template} />
<Story name="Eight inputs" args={{ inputs: 8, selected: 5 }} {template} />
