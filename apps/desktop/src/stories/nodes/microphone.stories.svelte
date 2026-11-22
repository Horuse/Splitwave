<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, signalArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs({ deviceId: { control: 'select', options: ['in-1', 'in-2', 'in-3', 'missing-device'] } }, { deviceId: 'in-1' }),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'signalMotion']),
		envArgs(['platform', 'running', 'inputDevices', 'deviceChannels', 'deviceSampleRate', 'deviceVolume', 'pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Inputs/Microphone', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="microphone" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="No device selected" args={{ deviceId: null }} {template} />
<Story name="No devices" args={{ deviceId: null, inputDevices: 0 }} {template} />
<Story name="Missing device" args={{ deviceId: 'missing-device' }} {template} />
<Story name="Hardware gain only" args={{ deviceVolume: 'unsupported' }} {template} />
<Story name="Resampling" args={{ deviceSampleRate: 44100 }} {template} />
<Story name="Multichannel interface" args={{ deviceChannels: 8, signalChannels: 8 }} {template} />
<Story name="Clipping" args={{ signalLevel: 0 }} {template} />
<Story name="Windows" args={{ platform: 'windows' }} {template} />
