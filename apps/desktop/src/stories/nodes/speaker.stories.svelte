<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, signalArgs, wiringArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs({ deviceId: { control: 'select', options: ['out-1', 'out-2', 'out-3', 'missing-device'] } }, { deviceId: 'out-1' }),
		wiringArgs(2),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'signalMotion']),
		envArgs(['platform', 'running', 'outputDevices', 'deviceChannels', 'deviceSampleRate', 'deviceVolume', 'pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Outputs/Speaker', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="speaker" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="No device selected" args={{ deviceId: null }} {template} />
<Story name="No devices" args={{ deviceId: null, outputDevices: 0 }} {template} />
<Story name="Missing device" args={{ deviceId: 'missing-device' }} {template} />
<Story name="Fixed volume" args={{ deviceVolume: 'unsupported' }} {template} />
<Story name="Volume not synced" args={{ deviceVolume: 'unsynced' }} {template} />
<Story name="Resampling" args={{ deviceSampleRate: 44100 }} {template} />
<Story name="Surround" args={{ deviceChannels: 6, wiredInputs: 6, signalChannels: 6 }} {template} />
<Story name="Unwired" args={{ wiredInputs: 0 }} {template} />
<Story name="Windows" args={{ platform: 'windows' }} {template} />
