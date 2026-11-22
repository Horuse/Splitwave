<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs(
			{
				name: { control: 'text' },
				codec: { control: 'inline-radio', options: ['opus', 'pcm-f32', 'pcm-i16'] },
				opusBitrate: { control: 'select', options: [32000, 64000, 96000, 128000] },
				opusApplication: { control: 'inline-radio', options: ['voip', 'audio', 'low-delay'] }
			},
			{ name: 'Studio A', codec: 'opus', opusBitrate: 96000, opusApplication: 'voip' }
		),
		wiringArgs(1),
		envArgs(['running', 'webrtcPhase', 'webrtcPeers', 'link', 'pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Network/WebRTC', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="webRtcCollaborator" args={a} />
{/snippet}

<Story name="Idle" {template} />
<Story name="Hosting, waiting" args={{ webrtcPhase: 'hosting' }} {template} />
<Story name="Hosting with peers" args={{ webrtcPhase: 'hosting', webrtcPeers: 3, running: true }} {template} />
<Story name="Joined" args={{ webrtcPhase: 'joining', webrtcPeers: 1, running: true }} {template} />
<Story name="Lossy link" args={{ webrtcPhase: 'hosting', webrtcPeers: 2, running: true, link: 'lossy' }} {template} />
<Story name="PCM" args={{ codec: 'pcm-f32' }} {template} />
<Story name="Stereo send" args={{ wiredInputs: 2 }} {template} />
<Story name="Resampling note" args={{ pipelineSampleRate: 44100 }} {template} />
