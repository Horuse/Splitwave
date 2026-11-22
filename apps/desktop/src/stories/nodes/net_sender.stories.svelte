<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				targetIp: { control: 'text' },
				port: { control: range(1024, 65535, 1) },
				codec: { control: 'inline-radio', options: ['opus', 'pcm-f32', 'pcm-i16'] },
				opusBitrate: { control: 'select', options: [32000, 64000, 96000, 128000] },
				opusApplication: { control: 'inline-radio', options: ['voip', 'audio', 'low-delay'] },
				sampleRate: { control: 'select', options: ['auto', 44100, 48000, 88200, 96000, 22050], mapping: { auto: null } }
			},
			{ targetIp: '192.168.1.20', port: 5004, codec: 'opus', opusBitrate: 96000, opusApplication: 'audio', sampleRate: 'auto' }
		),
		wiringArgs(2),
		envArgs(['running', 'link', 'pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Network/Net Sender', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="netSender" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="No target" args={{ targetIp: '' }} {template} />
<Story name="Sending" args={{ running: true }} {template} />
<Story name="PCM 96 kHz" args={{ codec: 'pcm-f32', sampleRate: 96000 }} {template} />
<Story name="PCM custom rate" args={{ codec: 'pcm-i16', sampleRate: 22050 }} {template} />
<Story name="Resampling note" args={{ pipelineSampleRate: 44100 }} {template} />
<Story name="Unwired" args={{ wiredInputs: 0 }} {template} />
