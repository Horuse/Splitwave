<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import ControlCanvas from '../harness/control_canvas.svelte';
	import { compose, dataArgs, range, wiringArgs } from '../harness/args';
	import { CONTROL_OUT, selectHandle } from '$lib/modules/flow/utils';

	const { argTypes, args } = compose(
		dataArgs({ outputs: { control: range(2, 8, 1) }, selected: { control: range(1, 8, 1) } }, { outputs: 3, selected: 1 }),
		wiringArgs(2)
	);

	const { Story } = defineMeta({ title: 'Nodes/Conditions/Switch', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="switch" args={a} />
{/snippet}

{#snippet driven()}
	<ControlCanvas from="toggle" fromData={{ on: true }} fromHandle={CONTROL_OUT} to="switch" toHandle={selectHandle(2)} />
{/snippet}

<Story name="Three outputs" {template} />
<Story name="Second open" args={{ selected: 2 }} {template} />
<Story name="Eight outputs" args={{ outputs: 8, selected: 5 }} {template} />
<Story name="Unwired" args={{ wiredInputs: 0 }} {template} />
<Story name="Picked by a trigger" template={driven} />
