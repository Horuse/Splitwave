<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				timeMs: { control: range(1, 2000, 1) },
				feedback: { control: range(0, 0.95, 0.01) },
				mix: { control: range(0, 1, 0.01) },
				bypassed: { control: 'boolean' }
			},
			{ timeMs: 250, feedback: 0.4, mix: 0.35, bypassed: false }
		),
		wiringArgs(2)
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Delay', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="delay" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Slapback" args={{ timeMs: 90, feedback: 0, mix: 0.3 }} {template} />
<Story name="Long tail" args={{ timeMs: 2000, feedback: 0.95, mix: 1 }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
