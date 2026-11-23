<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, signalArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs({ port: { control: range(1024, 65535, 1) } }, { port: 5004 }),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'signalMotion']),
		envArgs(['running', 'link', 'linkChannels', 'pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Network/Net Receiver', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="netReceiver" args={a} />
{/snippet}

<Story name="Offline" args={{ link: 'offline' }} {template} />
<Story name="Receiving" args={{ running: true }} {template} />
<Story name="Lossy" args={{ running: true, link: 'lossy' }} {template} />
<Story name="Multichannel" args={{ running: true, linkChannels: 6, signalChannels: 6 }} {template} />
<Story name="Resampling note" args={{ running: true, pipelineSampleRate: 44100 }} {template} />
