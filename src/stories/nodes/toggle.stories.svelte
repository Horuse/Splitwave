<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs } from '../harness/args';

	const { argTypes, args } = compose(dataArgs({ on: { control: 'boolean' } }, { on: false }));

	const { Story } = defineMeta({ title: 'Nodes/Conditions/Toggle', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="toggle" args={a} />
{/snippet}

<Story name="Off" {template} />
<Story name="On" args={{ on: true }} {template} />
