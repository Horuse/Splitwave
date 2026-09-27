<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				thresholdDb: { control: range(-80, 0, 0.5) },
				rangeDb: { control: range(-80, 0, 0.5) },
				attackMs: { control: range(0.1, 50, 0.1) },
				holdMs: { control: range(0, 500, 5) },
				releaseMs: { control: range(10, 1000, 5) },
				bypassed: { control: 'boolean' }
			},
			{ thresholdDb: -40, rangeDb: -40, attackMs: 1, holdMs: 50, releaseMs: 200, bypassed: false }
		),
		wiringArgs(2),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'gainReduction', 'signalMotion'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Noise Gate', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="noiseGate" args={a} />
{/snippet}

<Story name="Open" args={{ gainReduction: 0, signalMotion: false }} {template} />
<Story name="Hold" args={{ gainReduction: 6, signalMotion: false }} {template} />
<Story name="Closed" args={{ gainReduction: 24, signalMotion: false }} {template} />
<Story name="Chattering" args={{ gainReduction: 24 }} {template} />
<Story name="Soft range" args={{ rangeDb: -10 }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
