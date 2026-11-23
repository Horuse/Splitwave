<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, signalArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				filePath: {
					control: 'select',
					options: ['/Users/demo/Music/demo-track.wav', '/Users/demo/Music/a very long file name for a podcast episode recording.flac']
				},
				loopEnabled: { control: 'boolean' },
				autoStart: { control: 'boolean' },
				volume: { control: range(0, 1, 0.01) }
			},
			{ filePath: '/Users/demo/Music/demo-track.wav', loopEnabled: false, autoStart: true, volume: 1 }
		),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'signalMotion']),
		envArgs(['running', 'missingFile', 'pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Inputs/Audio File', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="audioFile" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Playing" args={{ running: true }} {template} />
<Story name="Looping" args={{ running: true, loopEnabled: true }} {template} />
<Story name="No file" args={{ filePath: null }} {template} />
<Story name="Missing file" args={{ missingFile: true }} {template} />
<Story name="Long name" args={{ filePath: '/Users/demo/Music/a very long file name for a podcast episode recording.flac' }} {template} />
<Story name="Resampling" args={{ running: true, pipelineSampleRate: 48000 }} {template} />
