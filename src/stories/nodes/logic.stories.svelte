<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs } from '../harness/args';
	import { LOGIC_OPS } from '@splitwave/nodes';

	const { argTypes, args } = compose(dataArgs({ op: { control: 'select', options: LOGIC_OPS.map((o) => o.op) } }, { op: 'and' }));

	const { Story } = defineMeta({ title: 'Nodes/Control/Logic', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="logic" args={a} />
{/snippet}

<Story name="And" {template} />
<Story name="Not" args={{ op: 'not' }} {template} />
