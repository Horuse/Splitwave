<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(dataArgs({ smoothing: { control: range(0, 1, 0.05) } }, { smoothing: 0.5 }), wiringArgs(2), signalArgs());

	const { Story } = defineMeta({ title: 'Nodes/Monitors/Spectrum', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="spectrum" args={a} />
{/snippet}

<Story name="Music" {template} />
<Story name="Sine 1 kHz" args={{ signal: 'sine', signalFrequency: 1000 }} {template} />
<Story name="Pink-ish noise" args={{ signal: 'noise' }} {template} />
<Story name="Sweep" args={{ signal: 'sweep' }} {template} />
<Story name="Fast ballistics" args={{ smoothing: 0 }} {template} />
<Story name="Slow ballistics" args={{ smoothing: 1 }} {template} />
<Story name="Silence" args={{ signal: 'silence' }} {template} />
<Story name="No signal" args={{ signal: 'off' }} {template} />
