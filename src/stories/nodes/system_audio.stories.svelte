<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import NodeCanvas from '../harness/node_canvas.svelte';
	import { compose, dataArgs, envArgs, range, signalArgs } from '../harness/args';

	const { argTypes, args } = compose(
		dataArgs({ volume: { control: range(0, 1, 0.01) }, excludeCurrentApp: { control: 'boolean' } }, { volume: 1, excludeCurrentApp: true }),
		signalArgs(['signal', 'signalLevel', 'signalChannels', 'signalMotion']),
		envArgs(['platform', 'running', 'deviceChannels', 'deviceSampleRate', 'capturePermission', 'pipelineSampleRate'])
	);

	const { Story } = defineMeta({ title: 'Nodes/Inputs/System Audio', argTypes, args });
</script>

{#snippet template(a: Record<string, unknown>)}
	<NodeCanvas kind="systemAudio" args={a} />
{/snippet}

<Story name="Default" {template} />
<Story name="Low volume" args={{ volume: 0.25 }} {template} />
<Story name="Screen recording denied" args={{ capturePermission: 'denied' }} {template} />
<Story name="Screen recording allowed" args={{ capturePermission: 'allowed' }} {template} />
<Story name="Resampling" args={{ deviceSampleRate: 44100 }} {template} />
<Story name="Surround" args={{ deviceChannels: 6, signalChannels: 6 }} {template} />
<Story name="Windows" args={{ platform: 'windows' }} {template} />
<Story name="Linux" args={{ platform: 'linux' }} {template} />
<Story name="Silence" args={{ signal: 'silence' }} {template} />
