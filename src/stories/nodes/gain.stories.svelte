<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs({ gainDb: { control: range(-24, 24, 0.1) }, bypassed: { control: 'boolean' } }, { gainDb: 0, bypassed: false }),
		wiringArgs(2),
		signalArgs(['signal', 'signalLevel', 'signalChannels']),
		envArgs(['running'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Gain', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="gain" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Boost" args={{ gainDb: 6 }} {template} />
<Story name="Hot" args={{ gainDb: 18 }} {template} />
<Story name="Cut" args={{ gainDb: -24 }} {template} />
<Story name="Bypassed" args={{ gainDb: 6, bypassed: true }} {template} />
<Story name="Unwired" args={{ wiredInputs: 0 }} {template} />
<Story name="Eight channels" args={{ wiredInputs: 8, signalChannels: 8 }} {template} />
