<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import ControlCanvas from '../harness/control_canvas.svelte';
	import { compose, dataArgs, wiringArgs } from '../harness/args';
	import { A_HANDLE, CONTROL_OUT } from '$lib/modules/flow/utils';

	const { argTypes, args } = compose(
		dataArgs(
			{
				selected: { control: 'inline-radio', options: [1, 2] },
				op: { control: 'select', options: ['a', 'not', 'and', 'or', 'xor', 'nand', 'nor', 'xnor'] }
			},
			{ selected: 1, op: 'a' }
		),
		wiringArgs(2)
	);

	const { Story } = defineMeta({ title: 'Nodes/Conditions/If', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="if" args={a} />
{/snippet}

{#snippet driven(a: Record<string, unknown>)}
	{#key a.selected}
		<ControlCanvas from="toggle" fromData={{ on: a.selected === 1 }} fromHandle={CONTROL_OUT} to="if" toHandle={A_HANDLE} />
	{/key}
{/snippet}

<Story name="True open" {template} />
<Story name="False open" args={{ selected: 2 }} {template} />
<Story name="Unwired" args={{ wiredInputs: 0 }} {template} />
<Story name="Mono" args={{ wiredInputs: 1 }} {template} />
<Story name="XOR" args={{ op: 'xor' }} {template} />
<Story name="NAND" args={{ op: 'nand' }} {template} />
<Story name="Driven by a toggle" template={driven} />
