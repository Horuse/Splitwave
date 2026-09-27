<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{ sensitivity: { control: range(0, 1, 0.01) }, maxWidthMs: { control: range(0.3, 5, 0.1) }, bypassed: { control: 'boolean' } },
			{ sensitivity: 0.5, maxWidthMs: 2, bypassed: false }
		),
		wiringArgs(2)
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Declick', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="declick" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Aggressive" args={{ sensitivity: 1, maxWidthMs: 5 }} {template} />
<Story name="Subtle" args={{ sensitivity: 0.1, maxWidthMs: 0.3 }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
