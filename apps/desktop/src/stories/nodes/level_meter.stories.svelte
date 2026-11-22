<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(dataArgs({}, {}), wiringArgs(2), signalArgs());

	const { Story } = defineMeta({ title: 'Nodes/Monitors/Level Meter', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="levelMeter" args={a} />
{/snippet}

<Story name="Music" {template} />
<Story name="Sine" args={{ signal: 'sine' }} {template} />
<Story name="Quiet" args={{ signalLevel: -40 }} {template} />
<Story name="Clipping" args={{ signalLevel: 0, signalClipping: true }} {template} />
<Story name="Silence" args={{ signal: 'silence' }} {template} />
<Story name="Mono" args={{ wiredInputs: 1, signalChannels: 1 }} {template} />
<Story name="Eight channels" args={{ wiredInputs: 8, signalChannels: 8 }} {template} />
<Story name="No signal" args={{ signal: 'off' }} {template} />
