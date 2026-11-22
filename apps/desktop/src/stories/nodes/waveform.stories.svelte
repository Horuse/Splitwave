<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(dataArgs({ segs: { control: range(1, 16, 1) } }, { segs: 4 }), wiringArgs(2), signalArgs());

	const { Story } = defineMeta({ title: 'Nodes/Monitors/Waveform', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="waveform" args={a} />
{/snippet}

<Story name="Music" {template} />
<Story name="Sine" args={{ signal: 'sine', signalFrequency: 220 }} {template} />
<Story name="Noise" args={{ signal: 'noise' }} {template} />
<Story name="Impulses" args={{ signal: 'impulses' }} {template} />
<Story name="Sweep" args={{ signal: 'sweep' }} {template} />
<Story name="Clipping" args={{ signalLevel: 0, signalClipping: true }} {template} />
<Story name="Silence" args={{ signal: 'silence' }} {template} />
<Story name="Mono" args={{ wiredInputs: 1, signalChannels: 1 }} {template} />
<Story name="Four channels" args={{ wiredInputs: 4, signalChannels: 4 }} {template} />
<Story name="No signal" args={{ signal: 'off' }} {template} />
