<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				attenuationLimitDb: { control: range(0, 100, 1) },
				postFilterBeta: { control: range(0, 0.05, 0.005) },
				minThreshDb: { control: range(-15, 35, 1) },
				maxErbThreshDb: { control: range(-15, 35, 1) },
				maxDfThreshDb: { control: range(-15, 35, 1) },
				bypassed: { control: 'boolean' }
			},
			{ attenuationLimitDb: 100, postFilterBeta: 0, minThreshDb: -10, maxErbThreshDb: 30, maxDfThreshDb: 20, bypassed: false }
		),
		wiringArgs(2),
		envArgs(['pipelineSampleRate', 'running', 'bufferFrames', 'workingBlock'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Noise Suppressor', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="noiseSuppressor" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Gentle" args={{ attenuationLimitDb: 12 }} {template} />
<Story name="Post filter" args={{ postFilterBeta: 0.02 }} {template} />
<Story name="Resampling note" args={{ pipelineSampleRate: 44100 }} {template} />
<Story name="Larger block" args={{ running: true, bufferFrames: 64, workingBlock: 480 }} {template} />
<Story name="Bypassed" args={{ bypassed: true }} {template} />
