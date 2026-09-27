<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{ thresholdDb: { control: range(-24, 0, 0.1) }, driveDb: { control: range(0, 24, 0.1) }, bypassed: { control: 'boolean' } },
			{ thresholdDb: -0.3, driveDb: 0, bypassed: false }
		),
		wiringArgs(2),
		envArgs(['pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Effects/Saturator', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="saturator" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Warm" args={{ driveDb: 6, thresholdDb: -6 }} {template} />
<Story name="Crushed" args={{ driveDb: 24, thresholdDb: -24 }} {template} />
<Story name="Bypassed" args={{ driveDb: 12, bypassed: true }} {template} />
<Story name="Resampling note" args={{ pipelineSampleRate: 44100 }} {template} />
