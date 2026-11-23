<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, range, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				thresholdDb: { control: range(-60, 0, 0.5) },
				ratio: { control: range(1, 20, 0.1) },
				attackMs: { control: range(0.1, 100, 0.1) },
				releaseMs: { control: range(10, 1000, 5) },
				kneeDb: { control: range(0, 24, 0.5) },
				makeupDb: { control: range(0, 24, 0.1) },
				bypassed: { control: 'boolean' }
			},
			{ thresholdDb: -18, ratio: 3, attackMs: 10, releaseMs: 100, kneeDb: 6, makeupDb: 0, bypassed: false }
		),
		wiringArgs(2),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'gainReduction', 'signalMotion'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Compressor', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="compressor" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Gentle" args={{ ratio: 1.5, thresholdDb: -12, gainReduction: 1.5 }} {template} />
<Story name="Squashed" args={{ ratio: 20, thresholdDb: -40, kneeDb: 0, makeupDb: 12, gainReduction: 18 }} {template} />
<Story name="Hard knee" args={{ kneeDb: 0 }} {template} />
<Story name="Idle" args={{ gainReduction: 0 }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
