<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				profile: { control: 'select', options: ['free', 'ebu', 'bs1770', 'atsc', 'aes', 'apple', 'spotify', 'acx'] },
				target: { control: range(-30, -8, 0.5) }
			},
			{ profile: 'free', target: -14 }
		),
		wiringArgs(2),
		signalArgs()
	);

	const { Story } = defineMeta({ title: 'Nodes/Monitors/Loudness', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="lufsMeter" args={a} />
{/snippet}

<Story name="Free" {template} />
<Story name="EBU R128" args={{ profile: 'ebu', target: -23 }} {template} />
<Story name="Spotify" args={{ profile: 'spotify', target: -14 }} {template} />
<Story name="ACX" args={{ profile: 'acx' }} {template} />
<Story name="Too loud" args={{ profile: 'ebu', target: -23, signalLevel: 0 }} {template} />
<Story name="Clipping" args={{ signalLevel: 0, signalClipping: true }} {template} />
<Story name="Silence" args={{ signal: 'silence' }} {template} />
<Story name="Mono" args={{ wiredInputs: 1, signalChannels: 1 }} {template} />
